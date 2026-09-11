//! Versioned project and preset data structures.

/// Returns the crate name for baseline workspace diagnostics.
#[must_use]
pub const fn crate_name() -> &'static str {
    "project-format"
}
