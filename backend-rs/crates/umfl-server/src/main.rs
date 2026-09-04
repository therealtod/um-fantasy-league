//! Entry point.
//!
//! Graceful shutdown: stop accepting, let in-flight requests finish, and give
//! up after 30 seconds. `deploy/docker-compose.prod.yml` allows
//! `stop_grace_period: 35s`, so the timeout has to stay inside that.

use std::net::SocketAddr;
use std::time::Duration;

use tokio::signal;
use tracing_subscriber::EnvFilter;
use umfl_server::config::Config;
use umfl_server::{build_router, state::AppState};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = Config::from_env()?;
    init_tracing(&config);

    tracing::info!(profiles = ?config.profiles, port = config.port, "Starting um-fantasy-league");

    let port = config.port;
    let state = AppState::connect(config).await?;
    let app = build_router(state);

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!(%addr, "Listening");

    // `into_make_service_with_connect_info` is what gives the rate limiter a
    // peer address to key on. Without it every caller shares one bucket.
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await?;
    Ok(())
}

/// `logging.level.com.umfl` -- DEBUG under `dev`, INFO everywhere else.
/// `RUST_LOG` overrides, which is the same escape hatch `LOGGING_LEVEL_COM_UMFL`
/// was.
///
/// The *format* splits the same way the level does, and for the same reason:
/// `dev` is read by a human in a terminal, `prod` is read by whatever tails the
/// container. So `prod` emits one JSON object per line -- the shape a log
/// collector can query by field (`level`, `target`, and whatever the event
/// itself carries) instead of regexing a rendered line -- and everything else
/// keeps the human-readable renderer.
///
/// `LOG_FORMAT=json|text` overrides that, and is read straight off the
/// environment rather than through [`Config`] for the same reason `RUST_LOG`
/// is: it configures the subscriber, which has to exist before anything can be
/// logged about reading configuration. Any other value falls through to the
/// profile default, since there is no subscriber yet to warn on.
fn init_tracing(config: &Config) {
    let default = if config.profiles.iter().any(|p| p == "dev") {
        "umfl_server=debug,tower_http=debug,info"
    } else {
        "umfl_server=info,info"
    };
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default));

    let json = match std::env::var("LOG_FORMAT").as_deref().map(str::trim) {
        Ok("json") => true,
        Ok("text") => false,
        _ => config.is_prod(),
    };

    let builder = tracing_subscriber::fmt().with_env_filter(filter);
    if json {
        // `flatten_event` lifts the event's own fields to the top level, so a
        // collector indexes `addr` rather than `fields.addr`. `with_span_list`
        // stays off: the current span's fields are the useful half, and the
        // full ancestry would repeat them on every line for a process whose
        // spans are one request deep.
        builder
            .json()
            .flatten_event(true)
            .with_current_span(true)
            .with_span_list(false)
            .init();
    } else {
        builder.init();
    }
}

async fn shutdown_signal() {
    let ctrl_c = async { signal::ctrl_c().await.expect("install Ctrl+C handler") };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("install SIGTERM handler")
            .recv()
            .await;
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => {}
        () = terminate => {}
    }
    tracing::info!("Shutdown signal received; draining for up to 30s");

    // `axum::serve`'s graceful shutdown waits indefinitely for in-flight
    // requests. An open SSE stream is in-flight by definition, so without this
    // cap a single idle standings tab would hold the process past compose's
    // 35s grace period and earn it a SIGKILL.
    tokio::spawn(async {
        tokio::time::sleep(Duration::from_secs(30)).await;
        tracing::warn!("Graceful shutdown timed out after 30s; exiting");
        std::process::exit(0);
    });
}
