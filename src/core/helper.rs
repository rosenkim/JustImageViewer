use crate::math::Rect2D;
use crate::render::image_uploader::UploadedTexture;

pub fn copy_region_to_clipboard(
    selection: Option<Rect2D>,
    texture: Option<&UploadedTexture>,
) {
    let texture = match texture {
        Some(t) => t,
        None => return,
    };

    let img_w = texture.width as u32;
    let img_h = texture.height as u32;

    // Determine the pixel-space rectangle to copy.
    let (copy_x, copy_y, copy_w, copy_h) = match selection {
        Some(sel) => {
            let x = (sel.min.x.floor() as u32).min(img_w);
            let y = (sel.min.y.floor() as u32).min(img_h);
            let x2 = (sel.max.x.ceil() as u32).min(img_w);
            let y2 = (sel.max.y.ceil() as u32).min(img_h);
            let w = x2.saturating_sub(x);
            let h = y2.saturating_sub(y);
            (x, y, w, h)
        }
        None => (0, 0, img_w, img_h),
    };

    if copy_w == 0 || copy_h == 0 {
        log::warn!("Copy: zero-size region, skipping");
        return;
    }

    // Extract the sub-region row by row.
    let src_stride = img_w as usize * 4;
    let dst_stride = copy_w as usize * 4;
    let mut rgba = vec![0u8; copy_h as usize * dst_stride];
    for row in 0..copy_h as usize {
        let src_offset = ((copy_y as usize + row) * src_stride) + copy_x as usize * 4;
        let dst_offset = row * dst_stride;
        rgba[dst_offset..dst_offset + dst_stride]
            .copy_from_slice(&texture.pixels[src_offset..src_offset + dst_stride]);
    }

    let image_data = arboard::ImageData {
        width: copy_w as usize,
        height: copy_h as usize,
        bytes: std::borrow::Cow::Owned(rgba),
    };

    match arboard::Clipboard::new() {
        Ok(mut cb) => {
            if let Err(err) = cb.set_image(image_data) {
                log::error!("Clipboard copy failed: {err:#}");
            } else {
                log::info!("Copied {}x{} image to clipboard", copy_w, copy_h);
            }
        }
        Err(err) => log::error!("Failed to open clipboard: {err:#}"),
    }
}
