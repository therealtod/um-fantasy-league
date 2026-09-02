//! One container per test binary, one migrated template database, one clone of
//! it per test.
//!
//! **This is deliberately not the Kotlin harness.** `PostgresIntegrationTest`
//! wraps each test in a transaction and rolls it back, which works because
//! Spring's transaction manager is thread-bound: the service under test joins
//! the test's own transaction and sees its uncommitted fixtures. In Rust a
//! service calls `state.pool.begin()` and gets a *different connection*, which
//! would see none of it. Preserving rollback would mean threading
//! `&mut PgConnection` through every service signature -- distorting production
//! code to suit the harness. So each test gets a real, committed database of
//! its own instead, cloned from a template Postgres file-copies.
//!
//! Three caveats in `AGENTS.md` are consequences of the rollback design and
//! **do not apply here**, which is worth knowing before porting a test that
//! works around them:
//!
//! * `AFTER_COMMIT` paths actually run. `StandingsSseHub` no longer has to be
//!   verified by a pure unit test standing in for an integration one.
//! * The `MatchResultCache.invalidateAll()` `@BeforeEach` is unnecessary: the
//!   cache is per-`AppState`, and every test has its own.
//! * The suite may run in parallel -- the shared-cache hazard that forced the
//!   Kotlin suite sequential is gone. Mark `SELECT ... FOR UPDATE` tests
//!   `#[serial]` if one ever needs it.

// Shared scaffolding for fifteen independent task owners, so a helper is
// routinely written one merge before its first caller. `dead_code` here would
// only ever say "nobody has ported that feature's tests yet".
#![allow(dead_code)]

pub mod migrate;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::{SyncSender, sync_channel};
use std::sync::{Mutex as StdMutex, OnceLock};
use std::time::Duration;

use axum::Router;
use axum::body::{Body, Bytes};
use axum::http::{Request, StatusCode, header};

use http_body_util::BodyExt;
use sqlx::{AssertSqlSafe, Connection, Executor, PgConnection, PgPool};
use testcontainers_modules::postgres::Postgres;
use testcontainers_modules::testcontainers::ImageExt;
use testcontainers_modules::testcontainers::runners::AsyncRunner;
use tokio::sync::{Mutex, oneshot};
use tower::ServiceExt;

// `atexit(3)` declared by hand rather than pulling in the `libc` crate for one
// FFI call -- see `reap_container_at_exit` for why this is the one hook that
// still fires.
unsafe extern "C" {
    fn atexit(cb: extern "C" fn()) -> std::os::raw::c_int;
}

/// The container thread's shutdown request, sent from `reap_container_at_exit`.
///
/// The `SyncSender<()>` riding along is the reply address: the thread drops
/// the container -- which is what actually stops/removes it -- and then acks
/// on it, so the `atexit` hook can wait for the removal to really finish
/// (bounded; see below) instead of racing the process's own exit.
static SHUTDOWN: OnceLock<StdMutex<Option<oneshot::Sender<SyncSender<()>>>>> = OnceLock::new();

use umfl_server::config::Config;
use umfl_server::manager::Manager;
use umfl_server::state::AppState;

/// The dev/test credential. `DevManagerAuthenticationFilter` resolves it once,
/// at the filter level, and only when it is present.
pub const MANAGER_ID_HEADER: &str = "X-Manager-Id";

/// `docker-compose.yml` and `PostgresIntegrationTest` both pin 17.
const POSTGRES_IMAGE_TAG: &str = "17-alpine";
const TEMPLATE_DB: &str = "umfl_template";

/// Where the migration and seed locations resolve to, and how AppState is
/// pointed at a fresh database.
struct Template {
    /// `postgres://user:pass@host:port/` -- append a database name.
    base_url: String,
}

/// The container, started once per test binary and kept alive for the whole
/// process.
///
/// It is owned by a dedicated thread running its own Tokio runtime, and that is
/// the load-bearing part: `#[tokio::test]` builds and *drops* a runtime per
/// test, so a `ContainerAsync` initialised on the first test's runtime would
/// lose the background tasks its Docker client depends on the moment that test
/// finished. Tying the container's life to the process instead is what the
/// Kotlin harness achieves by starting its container in a static initialiser
/// -- but the JVM runs shutdown hooks on exit and a bare `cargo test` binary
/// does not, so getting there needs one more piece here: see
/// `reap_container_at_exit`.
///
/// The thread parks on `shutdown_rx` rather than `std::future::pending`.
/// Parking on `pending` would mean the container is never dropped at all --
/// not "dropped too late", *never*: it sits inside a `static`, and Rust does
/// not run destructors on statics, or on other threads' locals, when a
/// process exits, `cargo test`'s harness included. So nothing short of an
/// explicit signal into this thread, acted on while its runtime is still
/// alive, ever gets `ContainerAsync::drop` to run -- and testcontainers-rs
/// has no Ryuk-style reaper (unlike the Java/Kotlin client) to fall back on
/// if it doesn't.
fn template() -> &'static Template {
    static TEMPLATE: OnceLock<Template> = OnceLock::new();
    TEMPLATE.get_or_init(|| {
        let (ready, wait) = std::sync::mpsc::channel();
        let (shutdown_tx, shutdown_rx) = oneshot::channel::<SyncSender<()>>();
        SHUTDOWN
            .set(StdMutex::new(Some(shutdown_tx)))
            .unwrap_or_else(|_| panic!("template() only runs its OnceLock::get_or_init once"));

        std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("build the container thread's runtime");
            runtime.block_on(async move {
                let container = Postgres::default()
                    .with_tag(POSTGRES_IMAGE_TAG)
                    .start()
                    .await
                    .expect("start postgres -- is the Docker daemon running?");
                let host = container.get_host().await.expect("container host");
                let port = container
                    .get_host_port_ipv4(5432)
                    .await
                    .expect("container port");
                let base_url = format!("postgres://postgres:postgres@{host}:{port}/");
                migrate_template(&base_url).await;
                ready.send(base_url).expect("hand the base URL back");

                // Hold the container for the rest of the run, then -- once
                // `reap_container_at_exit` asks -- drop it right here, still
                // inside this thread's own runtime. `ContainerAsync::drop`
                // needs `tokio::runtime::Handle::current()` to actually talk
                // to Docker (`testcontainers::core::async_drop`), and this is
                // the only place in the process guaranteed to still have one
                // by the time the request arrives.
                if let Ok(ack) = shutdown_rx.await {
                    drop(container);
                    let _ = ack.send(());
                }
            })
        });

        // The one hook that still runs on the way out: `cargo test`'s own
        // harness ends the process via `std::process::exit`, which -- like
        // any path through libc's `exit(3)` -- skips every Rust destructor
        // on every thread, but still invokes callbacks registered with
        // `atexit(3)`. Registered once, since `template()`'s `OnceLock` only
        // reaches this line once.
        unsafe {
            atexit(reap_container_at_exit);
        }

        let base_url = wait
            .recv()
            .expect("container setup panicked -- see the panic above this one");
        Template { base_url }
    })
}

/// Asks the container thread to drop the container, and waits (briefly) for
/// it to confirm.
///
/// Runs from `atexit(3)` -- see `template()` -- on whatever thread the
/// process happens to be exiting from, with no Tokio runtime of its own, so
/// it can only signal and wait, never run the async removal itself. The wait
/// is bounded: if Docker is wedged and the container thread never acks,
/// this gives up rather than hanging the test binary's shutdown -- the same
/// leak as before this fix, in a case that was never the common one.
extern "C" fn reap_container_at_exit() {
    let Some(cell) = SHUTDOWN.get() else {
        return;
    };
    let Some(shutdown) = cell
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .take()
    else {
        return;
    };
    let (ack_tx, ack_rx) = sync_channel(0);
    if shutdown.send(ack_tx).is_err() {
        return;
    }
    let _ = ack_rx.recv_timeout(Duration::from_secs(10));
}

/// Migrates `umfl_template` once: schema, reference data **and** seed.
///
/// The seed is included because the `test` profile includes it -- every
/// integration test in the Kotlin suite asserts against those fixtures, so a
/// template without them would be a different oracle.
async fn migrate_template(base_url: &str) {
    let mut admin = connect(&format!("{base_url}postgres")).await;
    admin
        .execute(sqlx::raw_sql(AssertSqlSafe(format!(
            r#"create database "{TEMPLATE_DB}""#
        ))))
        .await
        .expect("create the template database");
    // Closed before the first clone: `CREATE DATABASE ... TEMPLATE t` refuses
    // to run while anything is connected to `t`.
    admin.close().await.expect("close the admin connection");

    let mut conn = connect(&format!("{base_url}{TEMPLATE_DB}")).await;
    let db = repo_root().join("db");
    let plan = migrate::plan(&db.join("migration"), Some(&db.join("seed")));
    migrate::run(&mut conn, &plan).await;
    conn.close().await.expect("close the migration connection");
}

async fn connect(url: &str) -> PgConnection {
    PgConnection::connect(url)
        .await
        .unwrap_or_else(|e| panic!("connect to {url}: {e}"))
}

/// The repository root, found by walking up from this crate until `db/migration`
/// appears.
///
/// Walked rather than hard-coded as `../../../..`: `db/` moved out of
/// `backend/src/main/resources` to the repository root so that the Kotlin
/// backend and this one migrate the *same* files, and a relative-hop count is
/// the thing that silently breaks the next time the tree is rearranged.
fn repo_root() -> &'static Path {
    static ROOT: OnceLock<PathBuf> = OnceLock::new();
    ROOT.get_or_init(|| {
        let from = Path::new(env!("CARGO_MANIFEST_DIR"));
        from.ancestors()
            .find(|dir| dir.join("db/migration").is_dir())
            .unwrap_or_else(|| panic!("no `db/migration` above {}", from.display()))
            .to_path_buf()
    })
}

/// An application over its own database, driven in-process.
pub struct TestApp {
    pub state: AppState,
    pub db_name: String,
    router: Router,
}

impl TestApp {
    /// A fresh database cloned from the template, and a real router over it.
    ///
    /// Postgres implements `TEMPLATE` as a file copy of an already-migrated
    /// database, so this costs a fraction of re-running the migrations.
    pub async fn spawn() -> Self {
        Self::spawn_with(|_| {}).await
    }

    /// A fresh app whose `AppState` is adjusted before the router is built.
    ///
    /// The one thing worth swapping is `scraper`, this crate's only trait and
    /// the seam `MatchImportServiceTest` used a `@MockitoBean` for -- the
    /// alternative is standing Playwright up inside the test suite. The hook is
    /// deliberately "mutate the state" rather than a scraper-shaped parameter,
    /// so the next seam (if one is ever justified) needs no second constructor.
    pub async fn spawn_with(adjust: impl FnOnce(&mut AppState)) -> Self {
        let (mut state, db_name) = Self::connect_state().await;
        adjust(&mut state);
        let router = umfl_server::build_router(state.clone());
        Self {
            state,
            db_name,
            router,
        }
    }

    async fn connect_state() -> (AppState, String) {
        let template = template();
        static NEXT: AtomicU32 = AtomicU32::new(1);
        let db_name = format!("umfl_test_{}", NEXT.fetch_add(1, Ordering::Relaxed));

        // Serialised: concurrent clones of one template are the one thing
        // Postgres will refuse ("source database is being accessed by other
        // users"). A `tokio::sync::Mutex` is not bound to a runtime, so a
        // `static` one works across the per-test runtimes. The clone is ~100ms
        // of I/O, so this is not the suite's bottleneck.
        static CLONE: Mutex<()> = Mutex::const_new(());
        let guard = CLONE.lock().await;
        let mut admin = connect(&format!("{}postgres", template.base_url)).await;
        admin
            .execute(sqlx::raw_sql(AssertSqlSafe(format!(
                r#"create database "{db_name}" template "{TEMPLATE_DB}""#
            ))))
            .await
            .unwrap_or_else(|e| panic!("clone {TEMPLATE_DB} into {db_name}: {e}"));
        admin.close().await.expect("close the admin connection");
        drop(guard);

        // `from_env` rather than a struct literal, so the harness inherits
        // whatever the real defaults become instead of pinning a stale copy of
        // them. Only what the harness genuinely decides is overridden.
        let mut config = Config::from_env().expect("configuration defaults are valid");
        config.database_url = format!("{}{db_name}", template.base_url);
        config.profiles = vec!["test".to_owned()];
        config.frontend_origin = None;

        let state = AppState::connect(config)
            .await
            .unwrap_or_else(|e| panic!("connect to {db_name}: {e}"));
        (state, db_name)
    }

    pub fn pool(&self) -> &PgPool {
        &self.state.pool
    }

    /// Drives the **real** router in-process. No socket, no port, no
    /// `axum-test` dependency to lag axum 0.8 -- and every layer
    /// `build_router` installs is in the path, which is the point.
    pub async fn oneshot(&self, request: Request<Body>) -> TestResponse {
        let response = self
            .router
            .clone()
            .oneshot(request)
            .await
            .expect("the router is infallible");
        let status = response.status();
        let content_type = response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);
        let body = response
            .into_body()
            .collect()
            .await
            .expect("collect the response body")
            .to_bytes();
        TestResponse {
            status,
            content_type,
            body,
        }
    }

    /// The status of a response whose body is deliberately **not** collected.
    ///
    /// `GET /api/tournaments/{id}/standings/stream` is an SSE endpoint: its
    /// body ends only when the client disconnects or the hour-long cap
    /// elapses, so [`Self::oneshot`], which collects to the last byte, hangs on
    /// it forever. A test that only asserts on the status must not ask for the
    /// body. Dropping the response here drops the stream, which is what a
    /// disconnecting browser does and what releases the hub's subscription.
    pub async fn oneshot_status(&self, request: Request<Body>) -> StatusCode {
        self.router
            .clone()
            .oneshot(request)
            .await
            .expect("the router is infallible")
            .status()
    }

    pub async fn get(&self, uri: &str) -> TestResponse {
        self.oneshot(
            Request::builder()
                .uri(uri)
                .body(Body::empty())
                .expect("well-formed request"),
        )
        .await
    }

    pub async fn post_json(&self, uri: &str, body: &serde_json::Value) -> TestResponse {
        self.oneshot(
            Request::builder()
                .method("POST")
                .uri(uri)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body.to_string()))
                .expect("well-formed request"),
        )
        .await
    }

    /// A request carrying a manager credential, for the routes that need one.
    ///
    /// `body` is omitted entirely when `None` rather than sent as `{}`: a GET
    /// with a body is not what the frontend sends, and the extractors under
    /// test read the `Content-Type` header.
    pub async fn send_as(
        &self,
        method: &str,
        uri: &str,
        manager_id: i64,
        body: Option<&serde_json::Value>,
    ) -> TestResponse {
        let mut builder = Request::builder()
            .method(method)
            .uri(uri)
            .header(MANAGER_ID_HEADER, manager_id.to_string());
        if body.is_some() {
            builder = builder.header(header::CONTENT_TYPE, "application/json");
        }
        let body = body.map_or_else(Body::empty, |b| Body::from(b.to_string()));
        self.oneshot(builder.body(body).expect("well-formed request"))
            .await
    }

    pub async fn get_as(&self, uri: &str, manager_id: i64) -> TestResponse {
        self.send_as("GET", uri, manager_id, None).await
    }

    /// A seeded manager, by handle rather than by id.
    ///
    /// `AGENTS.md` warns that the integration tests assert on the seed's
    /// numbers exactly; a handle is the stable half of that, and ids shift the
    /// moment the fixture gains a row.
    pub async fn manager(&self, handle: &str) -> Manager {
        let row = sqlx::query!(
            "select id, handle, display_name, auth_user_id, is_admin
             from managers where handle = $1",
            handle
        )
        .fetch_one(self.pool())
        .await
        .unwrap_or_else(|e| panic!("no seeded manager {handle}: {e}"));
        Manager {
            id: row.id,
            handle: row.handle,
            display_name: row.display_name,
            auth_user_id: row.auth_user_id,
            is_admin: row.is_admin,
        }
    }

    pub async fn tournament_id(&self, name: &str) -> i64 {
        sqlx::query_scalar!("select id from tournaments where name = $1", name)
            .fetch_one(self.pool())
            .await
            .unwrap_or_else(|e| panic!("no seeded tournament {name}: {e}"))
    }

    pub async fn hero_id(&self, name: &str) -> i64 {
        sqlx::query_scalar!("select id from heroes where name = $1", name)
            .fetch_one(self.pool())
            .await
            .unwrap_or_else(|e| panic!("no hero {name}: {e}"))
    }

    pub async fn hero_ids(&self, names: &[&str]) -> Vec<i64> {
        let mut ids = Vec::with_capacity(names.len());
        for name in names {
            ids.push(self.hero_id(name).await);
        }
        ids
    }
}

pub struct TestResponse {
    pub status: StatusCode,
    pub content_type: Option<String>,
    pub body: Bytes,
}

impl TestResponse {
    pub fn json(&self) -> serde_json::Value {
        serde_json::from_slice(&self.body)
            .unwrap_or_else(|e| panic!("not JSON: {e}\n{}", self.text()))
    }

    pub fn text(&self) -> &str {
        std::str::from_utf8(&self.body).unwrap_or("<not utf-8>")
    }

    /// `jackson.default-property-inclusion: non_null` as an assertion.
    ///
    /// A null field is **absent** from a Kotlin response body, never `null`, so
    /// one emitted `null` anywhere in a document is a contract break. This is
    /// the same walk the differential rig runs over every parity body; it is
    /// here so a feature test can make the check on its own endpoint without
    /// waiting for the rig.
    pub fn assert_no_json_nulls(&self) {
        fn walk(value: &serde_json::Value, path: &str, whole: &serde_json::Value) {
            match value {
                serde_json::Value::Null => {
                    panic!("null at `{path}` -- non_null omits, never emits\n{whole:#}")
                }
                serde_json::Value::Array(items) => {
                    for (i, item) in items.iter().enumerate() {
                        walk(item, &format!("{path}[{i}]"), whole);
                    }
                }
                serde_json::Value::Object(fields) => {
                    for (key, field) in fields {
                        walk(field, &format!("{path}.{key}"), whole);
                    }
                }
                _ => {}
            }
        }
        let body = self.json();
        walk(&body, "$", &body);
    }
}
