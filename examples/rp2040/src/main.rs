#![no_std]
#![no_main]

#[cfg(feature = "compressed")]
use asydis::{CompressedDisplayPartition, FlushResult, SharedCompressedDisplay};
use embassy_embedded_hal::shared_bus::asynch::spi::SpiDeviceWithConfig;
use embassy_executor::Spawner;
use embassy_rp::{
    gpio,
    peripherals::SPI0,
    spi,
    spi::{Async, Spi},
};
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, mutex::Mutex};
use embassy_time::{Delay, Duration, Timer};
use embedded_graphics::{
    geometry::Size,
    mono_font::{MonoTextStyle, ascii::FONT_10X20},
    pixelcolor::Rgb565,
    prelude::*,
    primitives::{Line, PrimitiveStyle, Rectangle, StyledDrawable},
    text::{Alignment, Baseline, Text, TextStyleBuilder},
};
use gpio::{Level, Output};
#[cfg(feature = "compressed")]
type DisplayPartition<D> = CompressedDisplayPartition<D>;
#[cfg(feature = "compressed")]
const CHUNK_HEIGHT: usize = SCREEN_HEIGHT / 4;
#[cfg(feature = "compressed")]
type SharedDisplay<D> = SharedCompressedDisplay<CHUNK_HEIGHT, D>;
#[cfg(not(feature = "compressed"))]
use asydis::{DisplayPartition, FlushResult, SharedDisplay};

use ssd1351::{
    builder::Builder,
    mode::GraphicsMode,
    prelude::*,
    properties::{DisplayRotation, DisplaySize},
};
use static_cell::StaticCell;
use {defmt_rtt as _, panic_probe as _};

extern crate alloc;
use embedded_alloc::LlffHeap as Heap;

#[global_allocator]
static HEAP: Heap = Heap::empty();

const SCREEN_WIDTH: usize = 128;
const SCREEN_HEIGHT: usize = 96;

const MEM_USAGE_TRACK_INTERVAL: Duration = Duration::from_millis(200);

const BUF_SIZE: usize = SCREEN_WIDTH * SCREEN_HEIGHT * 2;
static mut BUF: [u8; BUF_SIZE] = [0; BUF_SIZE];

#[cfg(feature = "compressed")]
const COMPRESSION_GAINS: usize = 8_000;
#[cfg(feature = "compressed")]
const HEAP_SIZE: usize = 2048 + BUF_SIZE - COMPRESSION_GAINS;
#[cfg(not(feature = "compressed"))]
const HEAP_SIZE: usize = 2048;

type SpiBusType<'b> = Spi<'b, embassy_rp::peripherals::SPI0, embassy_rp::spi::Async>;
static SPI_BUS: StaticCell<Mutex<CriticalSectionRawMutex, SpiBusType>> = StaticCell::new();

type DisplayType<'a, 'b, 'c> = GraphicsMode<
    SPIInterface<
        SpiDeviceWithConfig<'a, CriticalSectionRawMutex, Spi<'b, SPI0, Async>, Output<'c>>,
        Output<'c>,
    >,
>;
#[embassy_executor::task]
async fn monitor_memory_usage() {
    loop {
        defmt::info!("mem_usage: {}", HEAP.used());
        Timer::after(MEM_USAGE_TRACK_INTERVAL).await;
    }
}

pub async fn text_app(mut display: DisplayPartition<DisplayType<'_, '_, '_>>) {
    let character_style = MonoTextStyle::new(&FONT_10X20, Rgb565::WHITE);
    let text_style = TextStyleBuilder::new()
        .baseline(Baseline::Middle)
        .alignment(Alignment::Center)
        .build();

    loop {
        Text::with_text_style(
            "hello \n world",
            Point::new(30, (SCREEN_HEIGHT / 2 - 10) as i32),
            character_style,
            text_style,
        )
        .draw(&mut display)
        .await
        .unwrap();
        Timer::after_millis(500).await;
        display.clear(Rgb565::BLACK).await.unwrap();
        Timer::after_millis(500).await;
    }
}

async fn line_app(mut display: DisplayPartition<DisplayType<'_, '_, '_>>) -> () {
    loop {
        Line::new(
            Point::new(0, 0),
            Point::new((SCREEN_WIDTH / 2) as i32, SCREEN_HEIGHT as i32),
        )
        .draw_styled(&PrimitiveStyle::with_stroke(Rgb565::WHITE, 1), &mut display)
        .await
        .unwrap();
        Timer::after_millis(500).await;
        Line::new(
            Point::new(0, SCREEN_HEIGHT as i32),
            Point::new((SCREEN_WIDTH / 2) as i32, 0),
        )
        .draw_styled(&PrimitiveStyle::with_stroke(Rgb565::WHITE, 1), &mut display)
        .await
        .unwrap();
        Timer::after_millis(500).await;
        display.clear(Rgb565::BLACK).await.unwrap();
        Timer::after_millis(500).await;
    }
}

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    defmt::info!("hello from defmt");
    {
        use core::mem::MaybeUninit;
        static mut HEAP_MEM: [MaybeUninit<u8>; HEAP_SIZE] = [MaybeUninit::uninit(); HEAP_SIZE];
        #[allow(static_mut_refs)]
        unsafe {
            HEAP.init(HEAP_MEM.as_ptr() as usize, HEAP_SIZE)
        }
    }
    #[allow(static_mut_refs)]
    let ptr = unsafe { BUF.as_mut_ptr() } as *mut u16;
    assert_eq!(
        ptr.align_offset(::core::mem::align_of::<u16>()),
        0,
        "Misaligned pointer for u16"
    );
    spawner.spawn(monitor_memory_usage()).unwrap();

    let p = embassy_rp::init(Default::default());

    let mut config = spi::Config::default();
    config.frequency = 4_000_000;

    let clk = p.PIN_2;
    let mosi = p.PIN_3;
    let cs = Output::new(p.PIN_5, Level::Low);
    let dc = Output::new(p.PIN_6, Level::Low);
    let mut rst = Output::new(p.PIN_7, Level::Low);
    let tx_dma = p.DMA_CH0;
    let spi = Spi::new_txonly(p.SPI0, clk, mosi, tx_dma, config.clone());
    let spi_bus: Mutex<CriticalSectionRawMutex, _> = Mutex::new(spi);
    let spi_bus_ref: &'static mut Mutex<_, _> = SPI_BUS.init(spi_bus);
    let spi_device = SpiDeviceWithConfig::new(spi_bus_ref, cs, config);
    let interface = SPIInterface::new(spi_device, dc);

    #[allow(static_mut_refs)]
    let mut display: DisplayType = Builder::new()
        .with_rotation(DisplayRotation::Rotate0)
        .with_size(DisplaySize::Display128x96)
        .connect_interface(interface, unsafe { &mut BUF })
        .into();

    display.reset(&mut rst, &mut Delay).unwrap();
    display.init().await.unwrap();

    defmt::info!("display init done");

    let mut shared_display: SharedDisplay<DisplayType> = SharedDisplay::new(display, spawner);

    let left_rect = Rectangle::new(
        Point::new(0, 0),
        Size::new((SCREEN_WIDTH / 2) as u32, SCREEN_HEIGHT as u32),
    );
    let right_rect = Rectangle::new(
        Point::new((SCREEN_WIDTH / 2) as i32, 0),
        Size::new((SCREEN_WIDTH / 2) as u32, SCREEN_HEIGHT as u32),
    );
    shared_display
        .launch_new_app(text_app, left_rect)
        .await
        .unwrap();
    shared_display
        .launch_new_app(line_app, right_rect)
        .await
        .unwrap();

    #[cfg(feature = "compressed")]
    shared_display
        .run_flush_loop_with_completion(
            async |_display| FlushResult::Continue,
            Duration::from_millis(20),
        )
        .await;

    #[cfg(not(feature = "compressed"))]
    shared_display
        .run_flush_loop_with(
            async |display, area| {
                display.flush_area(&area).await;
                FlushResult::Continue
            },
            Duration::from_millis(20),
        )
        .await;
}
