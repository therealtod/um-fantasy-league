//! Entry point.
//!
//! `server.shutdown: graceful` with `spring.lifecycle.timeout-per-shutdown-phase: 30s`,
//! reproduced: stop accepting, let in-flight requests finish, and give up after
//! 30 seconds. `deploy/docker-compose.prod.yml` allows `stop_grace_period: 35s`,
//! so the timeout has to stay inside that.

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
    // peer address to key on -- the equivalent of `HttpServletRequest
    // .getRemoteAddr()`, which a servlet container supplied for free. Without
    // it every caller shares one bucket.
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
fn init_tracing(config: &Config) {
    let default = if config.profiles.iter().any(|p| p == "dev") {
        "umfl_server=debug,tower_http=debug,info"
    } else {
        "umfl_server=info,info"
    };
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default)),
        )
        .init();
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
