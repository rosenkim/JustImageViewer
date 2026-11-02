// DPI / font conversion factors (used for pt -> px conversion).
pub const LOGICAL_DPI: f32 = 96.0;
pub const POINTS_PER_INCH: f32 = 72.0;

// Maximum number of concurrent background image decode tasks.
pub const MAX_DECODE_SPAWNS: isize = 5;

// Default AppConfig values.
pub const DEFAULT_UI_FONT_SIZE_PT: f32 = 10.5;
pub const DEFAULT_UI_SCALE_FACTOR: f32 = 1.0;
pub const DEFAULT_MEDIA_CACHE_COUNT: usize = 32;
pub const DEFAULT_FOCUSED_FPS: u32 = 60;
pub const DEFAULT_UNFOCUSED_FPS: u32 = 5;
pub const DEFAULT_LIBRARY_WIDTH: f32 = 300.0;

// Default background colors (hex RGB).
pub const DEFAULT_BACKGROUND_COLOR1: &str = "#CCCCCC";
pub const DEFAULT_BACKGROUND_COLOR2: &str = "#FFFFFF";

// Decoded thumbnail image size (in pixels).
pub const DEFAULT_THUMBNAIL_IMAGE_SIZE: u32 = 128;
// Thumbnail cell size drawn in the Library panel (in pixels).
pub const DEFAULT_LIBRARY_THUMBNAIL_SIZE: f32 = 108.0;

// File format returned by the HTTP `/view` endpoint. Change this one value to use another format.
pub const VIEW_IMAGE_FORMAT: image::ImageFormat = image::ImageFormat::Png;
// Maximum number of `/view` requests waiting for the next frame.
pub const VIEW_CAPTURE_QUEUE_CAPACITY: usize = 16;
// How long `/view` waits for an image. Loading may be slow, and a hidden window may not draw.
pub const VIEW_CAPTURE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);
