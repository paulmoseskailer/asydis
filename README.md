# Async Shared Display

Enabling concurrent screen-sharing applications with embedded Rust.

Extends [`embedded-graphics`](https://github.com/embedded-graphics/embedded-graphics) to provide easy development of concurrent, async screen-sharing applications for any display.
Also includes an option for [integrated framebuffer compression](#integrated-framebuffer-compression).

## How to Run

See `examples/` for examples with the [`embedded-graphics-simulator` display](https://github.com/embedded-graphics/simulator).
For the simulator to work, SDL2 has to be installed, as described in the [simulator repository](https://github.com/embedded-graphics/simulator?tab=readme-ov-file#setup).

```
cargo run --example hello_world
```

For an example on the Raspberry Pi Pico, see [`examples/rp2040`](./examples/rp2040).
Examples don't terminate.

## Why are there no tests?

There are tests in the `core` subcrate.
In the top-level crate, testing would require an `embassy_executor::Spawner`.
Since I do not know of a convenient way to test code that expects a spawner, the examples serve as tests for now: if they run without crashing, the test is considered passed.

## How to add support for a new display type

In order to use any display, all that is required is to implement the [`SharableBufferedDisplay`](core/src/sharable_display.rs) trait for the display type.
The display needs to use a framebuffer and implement the async version of `DrawTarget` from [my fork of `embedded-graphics`](https://github.com/paulmoseskailer/embedded-graphics) (has no PR yet due to unresolved issues with providing both sync and async versions simultaneously).
The trait requires specifying how to access the framebuffer which is used by the toolkit to allow multiple partitions to do so concurrently.
Any display implementing `SharableBufferedDisplay` can then be shared by creating a `SharedDisplay::new(display)` and apps can be launched with `SharedDisplay::launch_new_app(app_fn, partition_area)`.

See my forks of [`embedded-graphics-simulator`](https://github.com/paulmoseskailer/simulator/blob/master/src/display.rs#L264) and [`ssd1351` display driver](https://github.com/paulmoseskailer/ssd1351/blob/async_draw/src/mode/graphics.rs#L239) for example implementations of the `SharableBufferedDisplay` type.
Examples on how to use the `SharedDisplay` (with the simulator) can be found in `examples/` (see [How to Run](#how-to-run)).

## Integrated Framebuffer Compression

To use integrated framebuffer compression (using RLE-encoding), a display needs to implement the [`CompressableDisplay`](./core/src/compressable_display.rs) trait (instead of `SharableBufferedDisplay`).
Instead of requiring access to an existing framebuffer, this trait expects a specification of how a slice of pixels should be drawn to the screen.
Then, [`SharedCompressedDisplay`](./src/toolkit_compressed.rs#L24) is almost a drop-in replacement for `SharedDisplay`.
The display is flushed chunk by chunk, where a chunk is a part of the screen, spanning its entire width and `CHUNK_HEIGHT` pixels height.
The smaller the chunk height, the lower the peak memory usage, but the longer every flush takes.
See the documentation for details and the example in [`examples/compressed_hello_world.rs`](./examples/compressed_hello_world.rs).


## Some Notes on Design Decisions

- `core` is a sub-crate because the toolkit uses nightly Rust and drivers need to implement the `SharableBufferedDisplay` Trait. They need to be able to do so without switching to nightly.
