use std::net::SocketAddr;

use axum::{routing::get, Router};
use tokio::net::TcpListener;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

/// Start optional local HTTP server.
///
/// - Binds to `127.0.0.1:{port}`
/// - Serves `GET /` -> `Hello, World.`
/// - Shuts down gracefully when `token` is cancelled
pub fn start(port: u16, token: CancellationToken) -> JoinHandle<()> {
    tokio::spawn(async move {
        let addr: SocketAddr = SocketAddr::from(([127, 0, 0, 1], port));

        let listener = match TcpListener::bind(addr).await {
            Ok(listener) => {
                log::info!("HTTP server bound to http://{}", addr);
                listener
            }
            Err(err) => {
                log::error!("Failed to bind HTTP server on http://{}: {}", addr, err);
                return;
            }
        };

        let app = Router::new().route("/", get(root));

        if let Err(err) = axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                token.cancelled().await;
                log::info!("HTTP server shutdown requested");
            })
            .await
        {
            log::error!("HTTP server exited with error: {}", err);
        } else {
            log::info!("HTTP server stopped gracefully");
        }
    })
}

async fn root() -> &'static str {
    "Hello, World."
}
