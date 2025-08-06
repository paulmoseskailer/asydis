//! shared-display-core contains the core components of [shared-display] that are required to add
//! screen-sharing support for a display driver.
//!
//! This crate should only be used by drivers that extend shared-display. Applications should instead depend on [shared-display] itself.
//!
//! This crate heavily relies on and builds on top of [embedded-graphics](https://crates.io/crates/embedded-graphics)
//! and various crates of the [embassy project](embassy.dev).
//!
//!
//!
//!
#![no_std]
#![warn(missing_docs)]
#![allow(async_fn_in_trait)]

mod sharable_display;
pub use sharable_display::*;

mod compressable_display;
mod compressed_buffer;
pub use compressable_display::*;
pub use compressed_buffer::*;
