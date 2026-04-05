// DPI / font conversion factors (used for pt -> px conversion).
pub const LOGICAL_DPI: f32 = 96.0;
pub const POINTS_PER_INCH: f32 = 72.0;

// Maximum number of concurrent background image decode tasks.
pub const MAX_DECODE_SPAWNS: isize = 5;

// Default AppConfig values.
pub const DEFAULT_UI_FONT_SIZE_PT: f32 = 10.5;
pub const DEFAULT_UI_SCALE_FACTOR: f32 = 1.0;
pub const DEFAULT_IMAGE_CACHE_COUNT: usize = 32;
pub const DEFAULT_FOCUSED_FPS: u32 = 60;
pub const DEFAULT_UNFOCUSED_FPS: u32 = 5;
pub const DEFAULT_LIBRARY_WIDTH: f32 = 300.0;

// Default background colors (hex RGB).
pub const DEFAULT_BACKGROUND_COLOR1: &str = "#CCCCCC";
pub const DEFAULT_BACKGROUND_COLOR2: &str = "#FFFFFF";
