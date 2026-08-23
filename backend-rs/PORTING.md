# PORTING.md

**Mandatory reading before you write a line in `backend-rs/`.**

This is a **port, not a redesign**. Fifteen tasks are being implemented by owners who never see
each other's code, against a contract that is already frozen. The rules below are the ones that
have to be applied *identically* by all of them, so they live here rather than in fifteen heads.

Your task's entry on the board names the specific Kotlin files that are its **oracle**. Read those,
read this, read the shared type modules. Nobody reads the whole repo.

---

## 1. What is frozen

| Frozen | Why |
|---|---|
| `frontend/src/api/types.ts` | The contract anchor. Every endpoint, field name, status code and problem body comes out byte-identical. The unchanged Vue frontend working against the Rust backend *is* the acceptance test. |
| `db/migration/*.sql`, `db/seed/*.sql` | Same Postgres, same schema, same seed. **No task edits a migration.** A schema question is a plan question. |
| Violation message strings | The frontend renders `ApiError.violations[].message` verbatim to the user. Copy them character for character out of the Kotlin, including the punctuation. |

Anything you think is a bug: **port it faithfully, then raise it.** A behaviour change smuggled into
a port makes the differential diff useless, and the differential diff is the only thing standing
between this rewrite and a silently wrong leaderboard. See §12 for the two already-agreed exceptions.

---

## 2. Crate layout, and the one rule the compiler enforces

```
crates/umfl-domain/   pure. serde + chrono + rust_decimal + indexmap + thiserror. NOTHING else.
crates/umfl-server/   axum + sqlx + all I/O.
```

`AGENTS.md` says domain rules live in pure objects with no persistence dependency, and that new
rules belong there. In Kotlin that is a convention a reviewer enforces. Here the manifest has no
`sqlx`, no `axum`, no `tokio`, no `reqwest` — so it is a **build error**.

**Do not add an I/O dependency to `umfl-domain` to make something convenient.** If a rule seems to
need the database, it needs its *inputs* passed in instead. That is the whole reason
`standings::fold` exists as a pure function.

`cargo test -p umfl-domain` is the fast gate: no Docker, no `DATABASE_URL`, milliseconds. It is the
direct equivalent of `./gradlew :backend:test --tests '*RosterPolicyTest'`.

## 3. File names carry the read/write split

`AGENTS.md`'s central convention, kept by filename instead of by class name:

| File | Was | Contains |
|---|---|---|
| `query.rs` | `*Query` / `*QueryRepository` (`JdbcClient` reads) | `sqlx::query_as!` read projections |
| `writer.rs` | `*Repository` (Spring Data JDBC) | aggregate writes |
| `pool_admin.rs` | `*AdminRepository` (`JdbcClient` writes) | composite-keyed link-table writes |
| `service.rs` / `admin_service.rs` | `@Service` + `@Transactional` | the transaction boundaries |

Free `async fn`s, not structs with injected dependencies — there is no DI container and nothing to
mock. **Introduce a trait only where the Kotlin had a genuine test seam.** There is exactly one:
`ScraperClient`. If you are reaching for a second, you are probably reproducing Spring rather than
porting it.

**DTOs live with their feature**, not in a shared `Dtos.kt`/`AdminDtos.kt`. This is a deliberate
deviation from the Kotlin layout and it exists to remove the worst merge contention between parallel
owners. The JSON shape is unchanged.

## 3a. Where the shared domain types actually live

**`umfl-domain` is complete** — T1, T3, T4, T5, T5b and T12's pure half have all landed, and no
remaining task adds a module to it. Everything below is a landmark that is *not* where the Kotlin
package layout would suggest, so check here before going looking:

- **`MetricContext` and `HeroRole` are in `umfl_domain::match_result`**, not in a scoring module.
  Kotlin declares them in `scoring/MatchMetrics.kt`, but `MatchResult::hero_contexts()` is their only
  constructor, so splitting them would make the two modules circular. The `MatchMetrics` owner (T4)
  imports them from `match_result`.
- **`round2` is `umfl_domain::rounding::round2`**, gated by `crates/umfl-domain/tests/round2_oracle.rs`
  against a JDK-printed fixture. It **panics** on NaN, an infinity, or a magnitude past ~7.9e28 —
  `BigDecimal.valueOf` throws on the first two, so both runtimes answer 500 there. Set
  `UMFL_ROUND2_ORACLE_CSV` to replay the full 154k-row sample instead of the committed slice.
- **The whole leaderboard fold is `umfl_domain::standings`** — `board()`, `ticker()` and the private
  `rank()`, plus the wire types (`StandingsBoard`, `StandingsRow`, `MetricColumn`, `TickerEntry`,
  `TickerGame`, `TickerGameSide`) and the fold's roster input (`EntryRoster`, `RosterHero`). This is
  the port's one structural change and it is deliberate: in Kotlin the arithmetic is only reachable
  through a `@Transactional` service, so ranking, the dense breakdown and the ticker's draft banking
  had no unit coverage at all. **T11 does not port the fold** — it opens the REPEATABLE READ snapshot,
  reads rules/matches/rosters, and calls these two functions.
- **`MatchRule` and its input types are `umfl_domain::match_policy`**, not `match_result`. T9 builds
  `MatchParticipantInput` / `MatchGameInput` / `MatchBanInput` from the request DTO and converts
  `MatchViolation` into `Violation` at the service boundary.
- **`NameResolver` and `scraped_timestamps::parse` are already ported**, in `umfl_domain::name_resolver`
  and `umfl_domain::scraped_timestamps`. T12 owns only the server half — the `ScraperClient` trait,
  `MatchImportService`, the preview DTOs and the 503 path.
- **`umfl-domain` has one dependency the §2 list does not name: `chrono-tz`.** It is the IANA zone
  table, needed because the source site renders `"22:00 CEST"` with no offset. Pure data and a
  lookup, not I/O — the invariant is intact, and it is not a precedent for anything that opens a
  socket or a file.

## 3b. What `umfl-server` already carries

The counterpart to §3a for the I/O crate, kept here because the original fifteen-task board was
never written to a file and the T-numbers above are all that survived of it. **This list is the
board now.** Add to it in the same commit that lands a feature.

Landed, with tests:

| Area | Modules | Routes |
|---|---|---|
| Plumbing | `config`, `state`, `error`, `http/{problem,extract,big_decimal}`, `ratelimit`, `auth/{authenticate,authorize,dev,supabase}` | — |
| Actuator | `api/actuator` | `GET /actuator/{health,info}` |
| Manager | `manager/{query,writer}` | `GET /api/me` |
| Hero catalogue & pool | `hero/{query,writer,pool_admin,admin_service}` | `GET /api/tournaments/{id}/heroes`, `GET`/`POST /api/admin/heroes`, `PUT /api/admin/heroes/{id}`, `GET`/`POST /api/admin/tournaments/{id}/heroes`, `PUT`/`DELETE …/heroes/{heroId}` |
| Tournaments & rosters | `tournament/{query,writer,service,admin_service}` | `GET /api/tournaments`, `GET /api/tournaments/{id}`, `POST …/entries`, `GET …/entries/me`, `PUT …/entries/me/slots`, `POST …/entries/me/lock`, `POST /api/admin/tournaments`, `PUT`/`DELETE /api/admin/tournaments/{id}` |
| Scoring rule sets | `scoring/{query,writer,admin_service}` | `GET`/`POST /api/admin/tournaments/{id}/scoring-rule-sets`, `PUT …/{ruleSetId}`, `POST …/{ruleSetId}/activate` |
| Board pool | `map/{query,writer,pool_admin,admin_service}` | `GET`/`POST /api/admin/maps`, `PUT /api/admin/maps/{id}`, `GET`/`POST /api/admin/tournaments/{id}/maps`, `PUT`/`DELETE …/maps/{mapId}` |
| Match results | `match/{query,writer,cache,admin_service}` | `GET`/`POST /api/admin/tournaments/{id}/matches`, `GET`/`PUT`/`DELETE …/matches/{matchId}` |
| Standings | `standings/{query,service,sse}` | `GET /api/tournaments/{id}/standings`, `GET …/matches`, `GET …/standings/stream` |
| Match import | `matchimport/{query,scraper,service}` | `POST /api/admin/tournaments/{id}/matches/import` |

**Nothing is left to port.** The admin halves of `hero` and `tournament` — `AdminHeroService`,
`HeroPoolAdminRepository`, `AdminTournamentService` — were the last entry on this board; `api/mod.rs`
now merges every route the Kotlin serves, transitively through `hero::routes()` and
`tournament::routes()` (each feature's admin endpoints are merged inside its own `routes()`, per §10 —
`api/mod.rs` itself never grew a new line for them). `AdminHeroService.update`'s rename announcement
is landed too — see the (now resolved) note below for the reasoning.

One thing surfaced while porting the hero pool's `SetHeroCostRequest`/`HeroPoolEntryRequest.cost` and
is worth recording for the next person who meets the same shape: like `capacity`/`rosterSize`/
`creditGrant`, Kotlin's `cost` carries only `@Positive`, no `@NotNull`, so an absent `cost` there is
a live 500 today. Deviation (a) covers only those three tournament fields by name, but it is really a
statement about the *shape* — a field with `@Positive` and no `@NotNull` is always a bug, not a
tournament-specific one — so `cost` was fixed the identical way rather than faithfully reproducing a
fourth instance of the same defect: a 400 `validation-failed` naming the field, with Hibernate's own
un-customised `@NotNull` message (`"must not be null"`) for the half of the check that was never
actually annotated. If a fifth field with this exact shape turns up outside this crate's remaining
scope, this paragraph is the precedent, not deviation (a)'s original three names.

### Two notes for whoever picks up the next one

**The `StandingsUpdateEvent` pair has no event bus, and is now a trio.** `match/admin_service.rs`
reproduces both of its Kotlin listeners as explicit calls at each of `record`/`correct`/`delete`:
`announce` before the commit (inside the writing transaction) and `announce_completed` after it,
committed *or* rolled back. **`announce_committed` is the third, and sits after the `outcome?`** at
each of those three sites — commit-only, because telling browsers "something changed" about a write
that rolled back would be a lie. All three are one line each; if a fourth write method is ever added
it needs all three, in that order.

**The SSE hub kept the Kotlin's caps and lost its two thread pools.** `MAX_SUBSCRIBERS_PER_TOURNAMENT`
200, `MAX_TOTAL_SUBSCRIBERS` 500, the 20s keep-alive and the one-hour cap are all constants in
`standings/sse.rs`, per the `umfl.*` invariant. The four dispatch threads and the keep-alive
scheduler have no counterpart: axum attaches the heartbeat to each response stream, and a slow
client backs up in its own task and its own `broadcast` receiver rather than behind a shared pool —
which is what those threads were buying. A subscription is released by `Drop` on the response body,
covering in one path all four sites Kotlin's `releaseEmitter` had to make idempotent. `subscribe`
returns an already-rendered `Response` because `keep_alive` wraps the stream in a type axum does not
export.

**The scraper is the crate's one trait, and the harness swaps it by mutating `AppState`.**
`TestApp::spawn_with(|state| …)` is the Rust answer to `@MockitoBean`, and `tests/it/match_import.rs`'s
`StubScraper` is the only implementor besides `HttpScraperClient`. The hook is state-shaped rather
than scraper-shaped on purpose: a second seam (if one is ever justified) needs no second constructor.
`ScraperProperties`' timeouts and allowed hosts became constants in `matchimport/scraper.rs` -- only
`scraper.base-url` was ever bound to an environment variable, and `Config` already carried it.

**`AdminHeroService.update`'s rename announcement is landed.** `map::admin_service::update` made the
two-phase `match_cache.invalidate_all()` call its `ReferenceDataRenamedEvent` bought, gated on the
name actually changing; `hero::admin_service::update` carries the identical pair, so a hero rename
invalidates the cache the same way a board rename already did.
`match_cache.rs`'s `a_hero_rename_needs_the_global_invalidation_to_be_seen` is the end-to-end case
that exercises it.

**A streaming route has no last byte, and the harness collects to one.** `TestApp::oneshot` drains the
body, so it hangs forever on `GET …/standings/stream`; `TestApp::oneshot_status` exists for the tests
that only assert a status, and `security.rs` uses it. A test that wants the *events* takes the
`Response` from the hub directly and reads `into_data_stream()` under a `tokio::time::timeout` --
see `standings.rs`'s `a_committed_match_write_pushes_an_update_to_an_open_stream`.

§13's headline assertion now lives in `tests/it/standings.rs`, asserted off the HTTP response rather
than off the service. One deferred piece is left: `roster_flow.rs`'s UMFL-06 case still drops the
Kotlin's cross-check against `StandingsQuery.rosters`, which `standings::query::rosters` can now
satisfy.

**One known gap, raised rather than smuggled in (PORTING.md §1).** `match/cache.rs`'s loader runs its
six assembly queries on a pooled connection in autocommit, so on a *miss* the assembled list is not
itself a single snapshot — the Kotlin's loader runs inside `StandingsService`'s REPEATABLE READ
transaction and is. The standings service cannot pass its transaction in: `get_or_load` takes a
`Fn() -> Fut` shared by every waiter, which is exactly what collapses the burst. Closing it means
`load` opening its own `repeatable read read only` transaction — a two-line change to another
feature's file, and a change rather than a port, so it is filed here instead of being made in the
standings commit.

## 4. Serialization — six rules, all of them wire contract

**4.1. Every `Option` field on a response type carries `skip_serializing_if = "Option::is_none"`.**

`application.yml` sets `jackson.default-property-inclusion: non_null`, so a null field is **absent**
from the body, not `null`. A single emitted `null` is a contract break. The differential rig has one
very cheap, very strong assertion for this: it walks every parity response body and fails on any
JSON `null` anywhere.

```rust
#[serde(skip_serializing_if = "Option::is_none")]
pub map_name: Option<String>,
```

**4.2. `IndexMap`, never `HashMap`.** Kotlin's `LinkedHashMap`, `buildMap`, `groupBy` and
`associateBy` all preserve encounter order and are iterated in it. `std::HashMap` does not, and the
resulting drift is per-process random. `serde_json` is built with `preserve_order` for the same
reason, so a `serde_json::Value` object also keeps insertion order.

**4.3. Timestamps go through `umfl_domain::time`.** `chrono`'s `to_rfc3339()` emits `+00:00` and
`to_rfc3339_opts` emits a *fixed* fractional precision. Java's `Instant.toString()` emits a literal
`Z` and 0, 3, 6 or 9 fractional digits *chosen by significance*. Neither chrono form matches.

```rust
#[serde(with = "umfl_domain::time::java_instant")]
pub played_at: DateTime<Utc>,

#[serde(with = "umfl_domain::time::java_instant_opt", skip_serializing_if = "Option::is_none")]
pub locked_at: Option<DateTime<Utc>>,
```

**4.4. Field order follows declaration order.** Keep struct fields in the same order as the Kotlin
data class. Key order is not semantically significant and the differential rig canonicalises object
keys, but matching costs nothing and makes a hand-diff readable.

**4.5. Enums serialize as their Kotlin constant name.** `#[serde(rename_all = "SCREAMING_SNAKE_CASE")]`
where the Rust spelling differs. `BanType::SelfBan` must render `"SELF_BAN"`.

**4.6. A `numeric` column goes through `crate::http::big_decimal`.** `rust_decimal`'s own serde
emits a JSON *string*; `types.ts` declares `coefficient: number`. Going via `f64` fixes the type and
breaks the scale — `numeric(10,4)` prints `0.7500` and `BigDecimal.toString()` keeps every digit,
where an `f64` hop prints `0.75`. The helper is a `RawValue` round trip in both directions, so the
scale the client *sent* also survives: `AdminScoringService.create` echoes the aggregate it saved
rather than a re-read row, so a posted `12.0` comes back `12.0`.

```rust
#[serde(with = "crate::http::big_decimal")]
pub coefficient: Decimal,

// A request field whose `@NotNull` is enforced by `garde` rather than the type:
#[serde(default, with = "crate::http::big_decimal::option")]
pub coefficient: Option<Decimal>,
```

## 5. Errors

**Every `ApiError` variant is already defined**, in `error.rs`, by T0. Convert into them; do not add
one. Adding a variant is a cross-cutting change that lands in every owner's merge, so it needs a
reason, not a convenience.

| Situation | Raise |
|---|---|
| Resource does not exist | `DomainError::not_found(msg)` → 404 |
| Well-formed but conflicts with state | `DomainError::conflict(msg)` → 409 |
| Deliberate capacity limit (SSE caps, scraper down) | `DomainError::service_unavailable(msg)` → 503 |
| Roster / match / scoring rules broken | `DomainError::RosterRule(v)` / `MatchRule(v)` / `ScoringRule(v)` → 422 |
| No credential on a gated route | `ApiError::Unauthorized` → 401 |
| Credential present, not enough | `ApiError::Forbidden` → 403 |
| Body failed its `garde` rules | `ApiError::Validation(fields)` → 400 |
| A constraint reached the database unvalidated | falls out of `From<sqlx::Error>` → 409 |

`Violation` is a plain `{ rule: String, message: String }`. Each policy owns its own rule enum and
converts at the boundary — which is exactly why T3 and T5 can define `RosterRule` and `MatchRule`
without either of them editing `error.rs`.

**Never return a bare status code or a plain-text body from a handler.** Every error this API
produces is an RFC 7807 document naming a `https://umfl.dev/problems/<slug>` type.

## 6. Extractors — never `axum::Json`, `Path` or `Query` in a handler

Use `http::extract::{AppJson, ValidJson, AppPath, AppQuery}`. A bare `axum::Json` rejects with
axum's plain-text body and no problem type, which is a wire change; the wrappers reject with the
problem document Spring's `ResponseEntityExceptionHandler` produced, carrying Spring's own `detail`
strings. This rule is grep-able on purpose:

```bash
grep -rn 'axum::Json<\|Json(\w*): *Json<\|Path(\|Query(' crates/umfl-server/src --include='*.rs'
```

`axum::Json` as a **response** type is fine — the ban is on the request side.

`ValidJson<T>` runs `garde`. Copy the Hibernate default message verbatim into the attribute, since
that string is what the client renders:

```rust
#[garde(range(min = 1), message = "must be greater than 0")]
pub roster_size: i32,
```

Hibernate's defaults you will need: `@Positive` → `must be greater than 0`, `@NotNull` →
`must not be null`, `@NotBlank` → `must not be blank`, `@Size` → `size must be between {min} and {max}`.

## 7. Transactions — the service owns it, the query takes an executor

```rust
// query.rs — takes any executor, so it composes inside a transaction or outside one
pub async fn rosters(db: impl PgExecutor<'_>, tournament_id: i64) -> sqlx::Result<Vec<RosterRow>>

// writer.rs — takes the connection, because it is part of somebody's transaction
pub async fn insert_match(tx: &mut PgConnection, m: &TournamentMatchWrite) -> sqlx::Result<i64>

// service.rs — the file that carried @Transactional is the file that calls begin()
pub async fn record(state: &AppState, req: RecordMatchRequest) -> ApiResult<MatchResult> {
    let mut tx = state.pool.begin().await?;
    /* ... */
    tx.commit().await?;
}
```

Keeping the boundary in exactly the method that carries `@Transactional` today is what makes the
port auditable file by file. It also makes one invariant checkable by inspection: **the single
outbound HTTP call must not hold a database connection** — `matchimport` never calls `pool.begin()`.

The pool is `max_connections(10)`, stated explicitly. That is HikariCP's default and therefore the
number `MatchResultCache`'s entire rationale is written against. Do not raise it to make a slow path
look faster.

`StandingsService` needs `set transaction isolation level repeatable read read only` as the **first
statement after `BEGIN`**. Anywhere else and it silently degrades to READ COMMITTED.

## 8. Ordering and sorting

`sort_unstable_*` is **banned** by `clippy.toml`, and the ban is enforced in CI. The standings
ranking (standard competition ranking: 1, 2, 2, 4) and the ticker's winner-first participant sort
both depend on everything they did *not* compare keeping the order the database returned it in. Use
`sort_by` / `sort_by_key`.

Ranking compares `f64` with exact `!=`, **no epsilon**. That is safe because every value has already
been through `round2`, and an epsilon would create ties the Kotlin does not have.

## 9. Rounding — read this before you touch a number

`ScoringEngine.round2` is `BigDecimal.valueOf(d).setScale(2, HALF_UP).toDouble()`. `BigDecimal
.valueOf(double)` is `new BigDecimal(Double.toString(d))` — the **shortest decimal string that
round-trips**, not the exact binary expansion. Rust's `f64` `Display` is shortest-round-trip too, so
the faithful port is:

```rust
Decimal::from_str(&format!("{value}")).round_dp_with_strategy(2, RoundingStrategy::MidpointAwayFromZero)
```

Three things that look like simplifications and are not:

- `MidpointAwayFromZero` **is** `HALF_UP`. `MidpointNearestEven` is `HALF_EVEN` and is wrong.
- The `format!` round-trip is deliberate. `Decimal::from_f64_retain` has different precision
  semantics and is not documented as shortest-round-trip.
- **Each metric is rounded before summation.** Folding in `Decimal` end to end would produce *better*
  numbers and *different* ones.

Coefficients are `numeric(10,4)`; decode them as `Decimal`, then `.to_f64()` **before** multiplying,
exactly as the Kotlin's `.toDouble()` does.

## 10. Routes

Each feature exports `pub fn routes() -> Router<AppState>`; `api/mod.rs` merges them, one line per
feature. That is deliberately the only file independent owners touch in common. Keep your addition
to one line.

`AppState` in `state.rs` is the other shared file. Append **one** field, construct it in **one** line,
and put the type itself in your own module.

## 11. sqlx workflow

`query_as!` is checked against a live schema at compile time and against `.sqlx/` in CI and in the
image build.

```bash
export DATABASE_URL=postgres://umfl:umfl@localhost:5433/umfl
cargo sqlx prepare --workspace -- --all-targets   # before every commit that touched a query
```

`.sqlx/` entries are hash-named and additive, so two owners preparing concurrently do not conflict.
**Forgetting to prepare compiles fine locally and breaks the image build** — this is the one new
failure mode the Rust build has that the Gradle build did not.

## 12. Deviations from the Kotlin — the complete list

Nothing else deviates. If you find yourself wanting another entry, raise it before writing it.

Two behaviours were **assumed wrong and corrected against the running backend** during T6 rather
than deviating — recorded here because both look like deviations if you meet them cold:

* **An unrouted path is 401/403, not 404.** `anyRequest().denyAll()` denies it in the filter chain
  long before Spring MVC could raise `NoResourceFoundException`. `GET /nope` is 401 anonymously and
  **403** authenticated; `GET /api/nope` is 401 anonymously and **404** authenticated, because
  `/api/**` merely demands an identity and then hands the request to the router. That last row is
  the one a per-route authorization layer cannot reproduce, and is why `authorize` is a single
  middleware over the raw path.
* **`/actuator/health` reports `groups`.** The real body is
  `{"groups":["liveness","readiness"],"status":"UP"}` — Boot's default group registry, not anything
  in `application.yml`.

| # | Deviation | Status |
|---|---|---|
| a | `capacity` / `rosterSize` / `creditGrant` carry `@Positive` but no `@NotNull`, so omitting one **500s** today. Rust emits **400 `validation-failed`** — what the Kotlin would have produced had the annotation been there. | **Fixed.** Allowlisted in the differential rig. |
| b | `GameResult.winner` is a Kotlin computed property Jackson emits as an undeclared JSON field. `MatchListAdmin.vue:121` derives the winner itself and never reads it. | **Preserved during the port**, on the **DTO** rather than the domain type. Removed in its own commit after parity is green — never mix "port" and "change". |
| c | Dev-profile `OPTIONS /api/**` 401s, because `DevSecurityConfig` has no `.cors()`. | **Fixed.** `CorsLayer` sits outside `authorize` in both profiles. Prod behaviour is unchanged when `FRONTEND_ORIGIN` is unset. |
| d | `RosterDto.lockedAt` is typed `string \| null` in `types.ts` but omitted by `non_null`. | **Preserved.** The field has been absent all along and the frontend copes; emitting `null` would be the actual wire change. Follow-up filed against `types.ts`. |
| e | Actuator answered `application/vnd.spring-boot.actuator.v3+json`; this answers `application/json`. | **Accepted.** Nothing reads the media type — both healthchecks are a `wget` and a `fetch(...).ok` — and it is not part of the frontend contract. |
| g | Filter-level rejections (401, 403, 429) answered `application/problem+json;charset=ISO-8859-1`; this answers `application/problem+json`. The charset is Tomcat's default applied to `response.writer`, not a decision — handler-level problems already carry no charset in both. Bodies are byte-identical (fixed ASCII sentences) and `client.ts` never reads the header, only `response.json()`. | **Accepted**, same category as (e). Reproducing `ISO-8859-1` would misdeclare the encoding and corrupt the first non-ASCII `detail` anyone adds. |
| f | A malformed **query parameter** produced `Failed to convert 'sinceMatchId' with value: 'abc'`. `serde_urlencoded` does not report which key it was deserialising. Status (400) and problem type (`bad-request`) match; only the `detail` sentence differs. | **Accepted**, allowlisted. See `http/extract.rs::AppQuery`. |

## 13. Testing

**Layer 1 — pure domain.** No Docker, no database. Port the Kotlin test classes near 1:1, with
`rstest` for the parameterised tables. This is the merge gate and where nearly all the value is.

**Layer 2 — integration, template database per test.** Note this is *not* the Kotlin harness.
`PostgresIntegrationTest` rolls back a transaction per test, which works only because Spring's
transaction manager is thread-bound; in Rust a service calling `pool.begin()` gets a **different
connection** and would never see the test's uncommitted fixtures. Preserving rollback would mean
threading `&mut PgConnection` through every service signature — distorting production code to suit
the harness.

So instead: one container per test binary, one migrated `umfl_template` database, and each test does
`CREATE DATABASE umfl_test_<n> TEMPLATE umfl_template` (Postgres file-copies it; 50–150 ms).

Three consequences, all good, and worth knowing because they delete caveats you will find in
`AGENTS.md`:

- **`AFTER_COMMIT` paths actually run.** `AGENTS.md`'s largest test caveat — that no such listener
  ever fires during the suite, forcing `StandingsSseHub` to be verified by a pure unit test —
  disappears.
- The `invalidateAll()` belt-and-braces `@BeforeEach` is unnecessary.
- **The suite can run in parallel.** The shared-cache hazard that forced the Kotlin suite sequential
  is gone. Mark the `SELECT … FOR UPDATE` tests `#[serial]`.

Put **every** integration test in one binary (`tests/it/main.rs` with `mod` declarations). Each
`tests/*.rs` is otherwise its own binary with its own container.

Drive through the real router with `tower::ServiceExt::oneshot` — no network, and no `axum-test`
dependency to lag axum 0.8.

**Exact-seed assertions port literally**, and are the sharpest parity check available. Above all,
`StandingsIntegrationTest`'s hand-derived leaderboard — **ArthurianLegend 100.00, NeonStrategist
79.75, SherlockMain 76.50, MythicMind 61.00**, ranks `[1, 2, 3, 4]`, `currentRound 3` — asserted with
exact `f64` equality. That single assertion is simultaneously the tripwire for `round2` drift,
`numeric(10,4)` decode drift and fold-order drift.

## 14. Before you open a PR

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo sqlx prepare --workspace -- --all-targets   # if you touched a query
cargo test --workspace
grep -rn 'HashMap' crates/ --include='*.rs'       # should be IndexMap
grep -rn 'Option<' crates/umfl-server/src --include='*.rs' | grep -v skip_serializing_if
```

Your deliverable is **"the files my task owns, plus their tests, green"** — checkable by someone who
has not read your work.
