use std::io;
use std::net::SocketAddr;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};

use axum::body::Body;
use axum::extract::{Path as AxumPath, State};
use axum::http::{header, HeaderValue, Response, StatusCode};
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Router;
use tokio::net::TcpListener;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::constants::VIEW_CAPTURE_TIMEOUT;
use crate::core::image_encoder::ImageEncoder;
use crate::core::view_capture::{ViewCaptureClient, ViewCaptureError};

#[derive(Debug, Default, Clone)]
pub struct SharedWebState {
    current_directory: Option<PathBuf>,
    selected_file: Option<PathBuf>,
}

pub type SharedWebStateHandle = Arc<RwLock<SharedWebState>>;

/// Create a shared state handle for the web server.
pub fn new_shared_state() -> SharedWebStateHandle {
    Arc::new(RwLock::new(SharedWebState::default()))
}

/// Update current directory used by `/fs/*path`.
pub fn set_current_directory(state: &SharedWebStateHandle, directory: Option<PathBuf>) {
    if let Ok(mut guard) = state.write() {
        guard.current_directory = directory;
    } else {
        log::error!("Failed to lock web shared state for writing current directory");
    }
}

/// Update selected file used by `/select`.
pub fn set_selected_file(state: &SharedWebStateHandle, selected_file: Option<PathBuf>) {
    if let Ok(mut guard) = state.write() {
        guard.selected_file = selected_file;
    } else {
        log::error!("Failed to lock web shared state for writing selected file");
    }
}

/// Services used by `/view`.
#[derive(Clone)]
pub struct ViewRouteState {
    pub capture: ViewCaptureClient,
    pub encoder: Arc<dyn ImageEncoder>,
}

/// Start optional local HTTP server.
///
/// - Binds to `127.0.0.1:{port}`
/// - Serves:
///   - `GET /` -> "Hello, World."
///   - `GET /fs/*path` -> file from current directory
///   - `GET /select` -> currently selected file
///   - `GET /view` -> full-resolution image shown by the view panel
/// - Shuts down gracefully when `token` is cancelled
pub fn start(
    port: u16,
    token: CancellationToken,
    shared_state: SharedWebStateHandle,
    view_state: ViewRouteState,
    listening: Arc<AtomicBool>,
) -> JoinHandle<()> {
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
        listening.store(true, Ordering::Release);

        let app = Router::new()
            .route("/", get(root))
            .route("/select", get(select_file))
            .route("/view", get(view_image).with_state(view_state))
            .route("/fs/{*path}", get(fs_file))
            .with_state(shared_state);

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
        listening.store(false, Ordering::Release);
    })
}

async fn root() -> &'static str {
    "Welcome to Just-Image-Viewer!"
}

async fn select_file(State(state): State<SharedWebStateHandle>) -> impl IntoResponse {
    let selected_file = match state.read() {
        Ok(guard) => guard.selected_file.clone(),
        Err(_) => {
            log::error!("Failed to lock web shared state for reading selected file");
            return (StatusCode::INTERNAL_SERVER_ERROR, "Internal state error").into_response();
        }
    };

    let Some(path) = selected_file else {
        return (StatusCode::NOT_FOUND, "No selected file").into_response();
    };

    file_response_from_path(path).await
}

async fn view_image(State(view): State<ViewRouteState>) -> Response<Body> {
    let image = match tokio::time::timeout(VIEW_CAPTURE_TIMEOUT, view.capture.capture()).await {
        Ok(Ok(image)) => image,
        Ok(Err(ViewCaptureError::Busy)) => {
            return (StatusCode::SERVICE_UNAVAILABLE, "View capture is busy").into_response();
        }
        Ok(Err(ViewCaptureError::Unavailable(reason))) => {
            return (StatusCode::NOT_FOUND, reason).into_response();
        }
        Ok(Err(err)) => {
            log::error!("Failed to capture view: {err}");
            return (StatusCode::INTERNAL_SERVER_ERROR, "Failed to capture view").into_response();
        }
        Err(_) => {
            return (StatusCode::SERVICE_UNAVAILABLE, "View image is not ready").into_response();
        }
    };

    // Encoding a large image is slow, so keep it away from the async workers.
    let encoder = view.encoder;
    let encoded = match tokio::task::spawn_blocking(move || encoder.encode(&image)).await {
        Ok(Ok(encoded)) => encoded,
        Ok(Err(err)) => {
            log::error!("Failed to encode view: {err:#}");
            return (StatusCode::INTERNAL_SERVER_ERROR, "Failed to encode view").into_response();
        }
        Err(err) => {
            log::error!("View encoding task stopped: {err}");
            return (StatusCode::INTERNAL_SERVER_ERROR, "Failed to encode view").into_response();
        }
    };

    let mut response = Response::new(Body::from(encoded.bytes));
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static(encoded.content_type),
    );
    // The view changes all the time, so clients must not reuse an old image.
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

async fn fs_file(
    State(state): State<SharedWebStateHandle>,
    AxumPath(requested_path): AxumPath<String>,
) -> impl IntoResponse {
    let current_directory = match state.read() {
        Ok(guard) => guard.current_directory.clone(),
        Err(_) => {
            log::error!("Failed to lock web shared state for reading current directory");
            return (StatusCode::INTERNAL_SERVER_ERROR, "Internal state error").into_response();
        }
    };

    let Some(base_dir) = current_directory else {
        return (StatusCode::NOT_FOUND, "No current directory").into_response();
    };

    let safe_relative = match normalize_relative_path(&requested_path) {
        Some(p) => p,
        None => return (StatusCode::BAD_REQUEST, "Invalid path").into_response(),
    };

    let candidate = base_dir.join(safe_relative);
    file_response_from_path(candidate).await
}

async fn file_response_from_path(path: PathBuf) -> Response<Body> {
    let metadata = match tokio::fs::metadata(&path).await {
        Ok(meta) => meta,
        Err(err) => return io_error_to_response(err, &path),
    };

    if !metadata.is_file() {
        return (StatusCode::NOT_FOUND, "File not found").into_response();
    }

    let bytes = match tokio::fs::read(&path).await {
        Ok(data) => data,
        Err(err) => return io_error_to_response(err, &path),
    };

    let content_type = guess_content_type(&path);
    let mut response = Response::new(Body::from(bytes));
    *response.status_mut() = StatusCode::OK;
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static(content_type),
    );
    response
}

fn io_error_to_response(err: io::Error, path: &Path) -> Response<Body> {
    use io::ErrorKind;

    match err.kind() {
        ErrorKind::NotFound => (StatusCode::NOT_FOUND, "File not found").into_response(),
        ErrorKind::PermissionDenied => {
            log::warn!("Permission denied while serving {}", path.display());
            (StatusCode::FORBIDDEN, "Permission denied").into_response()
        }
        _ => {
            log::error!("I/O error while serving {}: {}", path.display(), err);
            (StatusCode::INTERNAL_SERVER_ERROR, "I/O error").into_response()
        }
    }
}

fn normalize_relative_path(raw: &str) -> Option<PathBuf> {
    if raw.is_empty() {
        return None;
    }

    let candidate = Path::new(raw);
    if candidate.is_absolute() {
        return None;
    }

    let mut normalized = PathBuf::new();
    for component in candidate.components() {
        match component {
            Component::Normal(seg) => normalized.push(seg),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return None,
        }
    }

    if normalized.as_os_str().is_empty() {
        None
    } else {
        Some(normalized)
    }
}

fn guess_content_type(path: &Path) -> &'static str {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase());

    match ext.as_deref() {
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("bmp") => "image/bmp",
        Some("webp") => "image/webp",
        Some("tif") | Some("tiff") => "image/tiff",
        Some("tga") => "image/x-tga",
        Some("ico") => "image/x-icon",
        Some("pnm") => "image/x-portable-anymap",
        Some("dds") => "image/vnd-ms.dds",
        Some("ff") | Some("farbfeld") => "image/farbfeld",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::image_encoder::{EncodedImage, SharedRgbaImage};
    use crate::core::view_capture::{respond_all, view_capture_channel};

    struct FakeEncoder;

    impl ImageEncoder for FakeEncoder {
        fn encode(&self, _image: &SharedRgbaImage) -> anyhow::Result<EncodedImage> {
            Ok(EncodedImage {
                bytes: vec![1, 2, 3],
                content_type: "image/test",
            })
        }
    }

    /// Answer the next capture request with `result`, then return the response.
    async fn view_response(
        result: Result<SharedRgbaImage, ViewCaptureError>,
    ) -> Response<Body> {
        let (capture, mut queue) = view_capture_channel(1);
        let state = ViewRouteState {
            capture,
            encoder: Arc::new(FakeEncoder),
        };
        let request = tokio::spawn(view_image(State(state)));
        loop {
            let replies = queue.take_pending();
            if !replies.is_empty() {
                respond_all(replies, result);
                break;
            }
            tokio::task::yield_now().await;
        }
        request.await.unwrap()
    }

    #[tokio::test]
    async fn view_uses_the_type_chosen_by_the_encoder() {
        let response = view_response(Ok(SharedRgbaImage::from_raw(1, 1, vec![0; 4].into()).unwrap())).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[header::CONTENT_TYPE], "image/test");
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
        let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        assert_eq!(&body[..], [1, 2, 3]);
    }

    #[tokio::test]
    async fn view_errors_map_to_http_status() {
        let busy = view_response(Err(ViewCaptureError::Busy)).await;
        assert_eq!(busy.status(), StatusCode::SERVICE_UNAVAILABLE);
        let unavailable = view_response(Err(ViewCaptureError::Unavailable("none".to_owned()))).await;
        assert_eq!(unavailable.status(), StatusCode::NOT_FOUND);
        let failed = view_response(Err(ViewCaptureError::Failed("gpu".to_owned()))).await;
        assert_eq!(failed.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }
}
