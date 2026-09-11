//! Shared rendering infrastructure for interactive and headless clients.

/// Returns the crate name for baseline workspace diagnostics.
#[must_use]
pub const fn crate_name() -> &'static str {
    "render-core"
}
