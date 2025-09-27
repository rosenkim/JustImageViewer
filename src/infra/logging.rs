use std::sync::OnceLock;

use env_logger::Env;

static LOGGER_ONCE: OnceLock<()> = OnceLock::new();

/// Initialise the application logger with a lightweight, readable format.
pub fn init() {
    LOGGER_ONCE.get_or_init(|| {
        let env = Env::default().default_filter_or("info");
        let mut builder = env_logger::Builder::from_env(env);
        builder.format(|buf, record| {
            use std::io::Write;
            let timestamp = buf.timestamp_millis();
            writeln!(
                buf,
                "{ts} [{level:^5}] {target}: {msg}",
                ts = timestamp,
                level = record.level(),
                target = record.target(),
                msg = record.args()
            )
        });
        builder.init();
    });
}
