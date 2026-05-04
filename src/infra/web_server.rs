use std::io;
use std::net::SocketAddr;
use std::path::{Component, Path, PathBuf};
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

/// Start optional local HTTP server.
///
/// - Binds to `127.0.0.1:{port}`
/// - Serves:
///   - `GET /` -> "Hello, World."
///   - `GET /fs/*path` -> file from current directory
///   - `GET /select` -> currently selected file
/// - Shuts down gracefully when `token` is cancelled
pub fn start(
    port: u16,
    token: CancellationToken,
    shared_state: SharedWebStateHandle,
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

        let app = Router::new()
            .route("/", get(root))
            .route("/select", get(select_file))
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
