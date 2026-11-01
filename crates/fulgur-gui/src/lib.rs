//! Native workspace UI and bounded background jobs over the accepted pipeline.
pub mod app;
mod flat_view;
mod image_view;
pub mod job;
pub mod preview;
pub mod sets;
mod settings;

/// Version shown in the window title.
pub const VERSION: &str = concat!(
    env!("CARGO_PKG_VERSION_MAJOR"),
    ".",
    env!("CARGO_PKG_VERSION_MINOR")
);
