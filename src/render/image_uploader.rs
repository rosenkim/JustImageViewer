use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicIsize, Ordering},
        Arc,
    },
    thread,
    time::Duration,
};

use crate::core::image_loader::DecodedImage;
use crate::core::image_manager::ImageManager;
use crate::render::imgui_textures::ImguiTextures;
use anyhow::Result;
use imgui::TextureId;
use tokio::sync::oneshot;
use wgpu::{Device, Queue};

const MAX_DECODE_SPAWNS: isize = 5;
static ACTIVE_DECODE_SPAWNS: AtomicIsize = AtomicIsize::new(0);

pub struct UploadedTexture {
    pub id: TextureId,
    pub width: usize,
    pub height: usize,
    /// RGBA pixel data (row-major, 4 bytes per pixel).
    pub pixels: Arc<[u8]>,
}

struct TextureRecord {
    texture_id: TextureId,
    width: usize,
    height: usize,
    last_used: u32,
    /// RGBA pixel data retained for clipboard copy operations.
    pixels: Arc<[u8]>,
}

/// Successful decode result from the background thread.
struct DecodeOutput {
    decoded: DecodedImage,
    /// The raw file bytes, present only when the file was freshly read from disk
    /// (cache miss). None when the binary was already in ImageManager's cache.
    fresh_binary: Option<Vec<u8>>,
}

/// In-flight async decode job waiting for completion.
struct PendingDecode {
    path: PathBuf,
    receiver: oneshot::Receiver<Result<DecodeOutput>>,
}

pub struct ImageUploader {
    textures: HashMap<PathBuf, TextureRecord>,
    max_texture_size: u32,
    max_cache_size: usize,
    access_counter: u32,
    /// Currently running background decode task (at most one at a time).
    pending: Option<PendingDecode>
}

impl ImageUploader {
    /// Create texture cache and decoded image cache.
    pub fn new(
        max_texture_size: u32,
        max_cache_size: usize,
    ) -> Self {
        Self {
            textures: HashMap::new(),
            max_texture_size,
            max_cache_size,
            access_counter: 0,
            pending: None,
        }
    }

    /// If a cached texture already exists for `path`, return it immediately.
    pub fn get_cached(&mut self, path: &Path) -> Option<UploadedTexture> {
        let existing = self.textures.get_mut(path)?;
        self.access_counter += 1;
        existing.last_used = self.access_counter;
        Some(UploadedTexture {
            id: existing.texture_id,
            width: existing.width,
            height: existing.height,
            pixels: existing.pixels.clone(),
        })
    }

    /// Kick off a background decode for `path` using `tokio::spawn_blocking`.
    /// Cancels any previously pending decode.
    pub fn request_decode(&mut self, path: &Path, image_manager: &mut ImageManager) {
        // If already cached, no need to decode again.
        if self.textures.contains_key(path) {
            self.pending = None;
            return;
        }

        let owned_path = path.to_path_buf();

        // Try binary cache first (cheap clone), otherwise read file on the blocking thread.
        let cached_binary = image_manager.take_cached_binary(path);

        while ACTIVE_DECODE_SPAWNS.load(Ordering::Acquire) >= MAX_DECODE_SPAWNS {
            thread::sleep(Duration::from_millis(1));
        }
        ACTIVE_DECODE_SPAWNS.fetch_add(1, Ordering::AcqRel);

        let (tx, rx) = oneshot::channel();
        tokio::task::spawn_blocking(move || {
            struct SpawnCounterGuard;
            impl Drop for SpawnCounterGuard {
                fn drop(&mut self) {
                    ACTIVE_DECODE_SPAWNS.fetch_sub(1, Ordering::AcqRel);
                }
            }
            let _spawn_counter_guard = SpawnCounterGuard;

            let result = if let Some(binary) = cached_binary {
                // Binary already cached: decode only, no need to store again.
                decode_from_binary(&binary).map(|decoded| DecodeOutput {
                    decoded,
                    fresh_binary: None,
                })
            } else {
                // Cache miss: read file and keep binary for ImageManager.
                decode_from_file(&owned_path).map(|(decoded, binary)| DecodeOutput {
                    decoded,
                    fresh_binary: Some(binary),
                })
            };
            let _ = tx.send(result);
        });

        self.pending = Some(PendingDecode {
            path: path.to_path_buf(),
            receiver: rx,
        });

        log::debug!("Async decode requested: {}", path.display());
    }

    /// Cancel any in-flight background decode.
    pub fn cancel_pending(&mut self) {
        if let Some(pending) = self.pending.take() {
            log::debug!("Cancelled pending decode: {}", pending.path.display());
        }
    }

    /// Return the path of the currently pending decode, if any.
    pub fn pending_path(&self) -> Option<&Path> {
        self.pending.as_ref().map(|p| p.path.as_path())
    }

    /// Poll the background decode result. If ready, upload to GPU and return
    /// the texture together with the path it was decoded from.
    /// Call this once per frame from the main loop.
    pub fn poll_decoded(
        &mut self,
        device: &Device,
        queue: &Queue,
        renderer: &mut imgui_wgpu::Renderer,
        imgui_textures: &mut ImguiTextures,
        image_manager: &mut ImageManager,
    ) -> Option<(PathBuf, UploadedTexture)> {
        let pending = self.pending.as_mut()?;

        // Non-blocking check: is the decode finished?
        let output = match pending.receiver.try_recv() {
            Ok(Ok(output)) => output,
            Ok(Err(err)) => {
                let path = self.pending.take().unwrap().path;
                log::error!("Async decode failed for {}: {:#}", path.display(), err);
                return None;
            }
            // Channel closed unexpectedly (task panicked).
            Err(oneshot::error::TryRecvError::Closed) => {
                let path = self.pending.take().unwrap().path;
                log::error!("Async decode task dropped for {}", path.display());
                return None;
            }
            // Not ready yet, keep waiting.
            Err(oneshot::error::TryRecvError::Empty) => return None,
        };

        let path = self.pending.take().unwrap().path;

        // Feed freshly read binary back into ImageManager so future requests hit the cache.
        if let Some(binary) = output.fresh_binary {
            image_manager.store_binary(path.clone(), binary);
        }

        let decoded = output.decoded;

        if decoded.width as u32 > self.max_texture_size
            || decoded.height as u32 > self.max_texture_size
        {
            log::error!(
                "Image size {}x{} exceeds GPU max texture size {} for {}",
                decoded.width,
                decoded.height,
                self.max_texture_size,
                path.display()
            );
            return None;
        }

        let texture_id = match imgui_textures.create_from_rgba_data(
            device,
            queue,
            renderer,
            decoded.width as u32,
            decoded.height as u32,
            &decoded.pixels,
            true,
        ) {
            Ok(id) => id,
            Err(err) => {
                log::error!("GPU upload failed for {}: {:#}", path.display(), err);
                return None;
            }
        };

        self.evict_if_full(renderer, imgui_textures);

        self.access_counter += 1;
        let record = TextureRecord {
            texture_id,
            width: decoded.width,
            height: decoded.height,
            last_used: self.access_counter,
            pixels: decoded.pixels.clone(),
        };

        let result_path = path.clone();
        let uploaded = UploadedTexture {
            id: texture_id,
            width: decoded.width,
            height: decoded.height,
            pixels: decoded.pixels,
        };
        self.textures.insert(path, record);

        Some((result_path, uploaded))
    }

    /// Returns true if there is a background decode in progress.
    pub fn is_pending(&self) -> bool {
        self.pending.is_some()
    }

    /// Evict the least-recently-used entry if the cache is at capacity.
    fn evict_if_full(&mut self, renderer: &mut imgui_wgpu::Renderer, imgui_textures: &mut ImguiTextures) {
        if self.textures.len() < self.max_cache_size {
            return;
        }

        let oldest_key = self
            .textures
            .iter()
            .min_by_key(|(_, record)| record.last_used)
            .map(|(key, _)| key.clone());

        if let Some(key) = oldest_key {
            if let Some(record) = self.textures.remove(&key) {
                log::debug!("Evicting cached texture: {}", key.display());
                imgui_textures.remove(renderer, record.texture_id);
            }
        }
    }

    /// Free all GPU textures owned by this manager.
    pub fn clear(&mut self, renderer: &mut imgui_wgpu::Renderer, imgui_textures: &mut ImguiTextures) {
        self.pending = None;
        self.textures.clear();
        imgui_textures.clear(renderer);
    }
}

fn decode_from_binary(binary: &[u8]) -> Result<DecodedImage> {
    let dyn_image = image::load_from_memory(binary)?;
    let rgba_image = dyn_image.to_rgba8();
    let (width, height) = rgba_image.dimensions();
    let pixels = std::sync::Arc::<[u8]>::from(rgba_image.into_raw());

    Ok(DecodedImage {
        width: width as usize,
        height: height as usize,
        pixels,
    })
}

fn decode_from_file(path: &Path) -> Result<(DecodedImage, Vec<u8>)> {
    let binary = std::fs::read(path)?;
    let decoded = decode_from_binary(&binary)?;
    Ok((decoded, binary))
}

