use std::path::PathBuf;

use tokio::sync::mpsc;

use super::{DirectoryId, ThumbnailResult};
use crate::core::image_loader;
use crate::core::media::{MediaFormat, MediaType};

/// Decode thumbnails in the background and send each result to the viewer.
pub(super) async fn generate_thumbnails(
    directory_id: DirectoryId,
    paths: Vec<PathBuf>,
    thumbnail_size: u32,
    tx: mpsc::Sender<ThumbnailResult>,
) {
    for path in paths {
        let media_type = path
            .extension()
            .and_then(|ext| ext.to_str())
            .and_then(MediaFormat::from_extension)
            .map(|format| format.media_type());
        if media_type != Some(MediaType::Image2D) {
            continue;
        }

        // Decode off the async runtime because image codecs are blocking.
        let result = tokio::task::spawn_blocking({
            let path = path.clone();
            move || image_loader::load_thumbnail_rgba(&path, thumbnail_size)
        })
        .await;

        match result {
            Ok(Ok(decoded)) => {
                let msg = ThumbnailResult {
                    directory_id,
                    path,
                    width: decoded.width as u32,
                    height: decoded.height as u32,
                    pixels: decoded.pixels,
                };
                if tx.send(msg).await.is_err() {
                    break;
                }
            }
            Ok(Err(err)) => {
                log::warn!("Failed to load thumbnail for {}: {:#}", path.display(), err)
            }
            Err(err) => {
                log::warn!("Thumbnail task panicked for {}: {:#}", path.display(), err)
            }
        }
        tokio::task::yield_now().await;
    }
}
