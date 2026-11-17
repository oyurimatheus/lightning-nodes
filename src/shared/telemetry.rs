use tracing_subscriber::{EnvFilter, fmt, prelude::*};

/// Initialize structured logging. `RUST_LOG` controls the filter; the default
/// is `info`. Fields such as `sync_id`, `snapshot_id` and `duration_ms` are
/// emitted as structured events by callers.
pub fn init() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::registry()
        .with(fmt::layer().with_target(false).compact())
        .with(filter)
        .init();
}
