//! Provides an abstraction layer over [embedded-graphics](https://crates.io/crates/embedded-graphics), allowing to share a single display
//! among multiple apps, with async concurrency and memory safety handled by the toolkit.
//!
//!
//! # Examples
//!
//! More examples can be found in the [examples directory of the github repo](https://github.com/paulmoseskailer/shared-display/tree/main/examples).
//!
//! ## Sharing a [`SimulatorDisplay`](https://github.com/embedded-graphics/simulator)
//!
//! ```rust,no_run
//! # use embassy_executor::Spawner;
//! # use embassy_time::{Duration, Timer};
//! # use embedded_graphics::{
//! #    geometry::Size,
//! #    mono_font::{MonoTextStyle, ascii::FONT_10X20},
//! #    pixelcolor::BinaryColor,
//! #    prelude::*,
//! #    primitives::{Line, PrimitiveStyle, Rectangle, StyledDrawable},
//! #    text::{Alignment, Baseline, Text, TextStyleBuilder},
//! # };
//! # use embedded_graphics_simulator::{
//! #     BinaryColorTheme, OutputSettingsBuilder, SimulatorDisplay, SimulatorEvent, Window,
//! # };
//! use shared_display::{DisplayPartition, FlushResult, SharedDisplay};
//!
//! type DisplayType = SimulatorDisplay<BinaryColor>;
//!
//! fn init_simulator_display() -> (DisplayType, Window) {
//!     let output_settings = OutputSettingsBuilder::new()
//!         .theme(BinaryColorTheme::OledWhite)
//!         .build();
//!     (
//!         SimulatorDisplay::new(Size::new(128, 64)),
//!         Window::new("Simulated Display", &output_settings),
//!     )
//! }
//!
//! async fn text_app(mut display: DisplayPartition<DisplayType>) -> () {
//!     let character_style = MonoTextStyle::new(&FONT_10X20, BinaryColor::On);
//!     let text_style = TextStyleBuilder::new()
//!         .baseline(Baseline::Middle)
//!         .alignment(Alignment::Center)
//!         .build();
//!
//!     loop {
//!         Text::with_text_style(
//!             "hello \n world",
//!             Point::new(30, 20),
//!             character_style,
//!             text_style,
//!         )
//!         .draw(&mut display)
//!         .await
//!         .unwrap();
//!         Timer::after_millis(500).await;
//!         display.clear(BinaryColor::Off).await.unwrap();
//!         Timer::after_millis(500).await;
//!     }
//! }
//!
//! async fn line_app(mut display: DisplayPartition<DisplayType>) -> () {
//!     loop {
//!         Line::new(Point::new(0, 0), Point::new(128, 128))
//!             .draw_styled(
//!                 &PrimitiveStyle::with_stroke(BinaryColor::On, 1),
//!                 &mut display,
//!             )
//!             .await
//!             .unwrap();
//!         Timer::after_millis(500).await;
//!         Line::new(Point::new(0, 63), Point::new(63, 0))
//!             .draw_styled(
//!                 &PrimitiveStyle::with_stroke(BinaryColor::On, 1),
//!                 &mut display,
//!             )
//!             .await
//!             .unwrap();
//!         Timer::after_millis(500).await;
//!         display.clear(BinaryColor::Off).await.unwrap();
//!         Timer::after_millis(500).await;
//!     }
//! }
//!
//! #[embassy_executor::main]
//! async fn main(spawner: Spawner) {
//!     let (display, mut window) = init_simulator_display();
//!     let mut shared_display: SharedDisplay<DisplayType> = SharedDisplay::new(display, spawner);
//!
//!     let right_rect = Rectangle::new(Point::new(64, 0), Size::new(64, 64));
//!     shared_display
//!         .launch_new_app(line_app, right_rect)
//!         .await
//!         .unwrap();
//!
//!     let left_rect = Rectangle::new(Point::new(0, 0), Size::new(64, 64));
//!     shared_display
//!         .launch_new_app(text_app, left_rect)
//!         .await
//!         .unwrap();
//!
//!     shared_display
//!         .run_flush_loop_with(async |d, _area| {
//!             window.update(d);
//!             if window.events().any(|e| e == SimulatorEvent::Quit) {
//!                 return FlushResult::Abort;
//!             }
//!             FlushResult::Continue
//!         }, Duration::from_millis(200))
//!         .await;
//! }
//! ```
//!
//! ## Adding Support for a Screen driver
//!
//! To make a screen sharable, it needs to implement [`SharableBufferedDisplay`].
//! To make it usable with integrated framebuffer compression, it needs to implement
//! [`CompressableDisplay`].
//! See these forks of the
//! [`embedded-graphics-simulator`](https://github.com/paulmoseskailer/simulator) and the
//! [`ssd1351` screen driver](https://github.com/paulmoseskailer/ssd1351) for examples.
//!
//!
//!
//!
#![no_std]
#![feature(async_fn_traits)]
#![warn(missing_docs)]

mod toolkit;
mod toolkit_compressed;

pub use shared_display_core::*;
pub use toolkit::*;
pub use toolkit_compressed::*;
