use embassy_executor::Spawner;
use embassy_time::Timer;
use embedded_graphics::{
    geometry::Size,
    mono_font::{MonoTextStyle, ascii::FONT_10X20},
    pixelcolor::BinaryColor,
    prelude::*,
    primitives::{Line, PrimitiveStyle, Rectangle, StyledDrawable},
    text::{Alignment, Baseline, Text, TextStyleBuilder},
};
use embedded_graphics_simulator::{
    BinaryColorTheme, OutputSettingsBuilder, SimulatorDisplay, SimulatorEvent, Window,
};
use shared_display::{CompressedDisplayPartition, FlushResult, SharedCompressedDisplay};

type DisplayType = SimulatorDisplay<BinaryColor>;

const SCREEN_WIDTH: usize = 128;
const SCREEN_HEIGHT: usize = 96;

fn init_simulator_display() -> (DisplayType, Window) {
    let output_settings = OutputSettingsBuilder::new()
        .theme(BinaryColorTheme::OledWhite)
        .build();
    (
        SimulatorDisplay::new(Size::new(SCREEN_WIDTH as u32, SCREEN_HEIGHT as u32)),
        Window::new("Simulated Display", &output_settings),
    )
}

async fn text_app(mut display: CompressedDisplayPartition<DisplayType>) -> () {
    let character_style = MonoTextStyle::new(&FONT_10X20, BinaryColor::On);
    let text_style = TextStyleBuilder::new()
        .baseline(Baseline::Middle)
        .alignment(Alignment::Center)
        .build();

    loop {
        Text::with_text_style(
            "hello \n world",
            Point::new(SCREEN_WIDTH as i32 / 4, SCREEN_HEIGHT as i32 / 3),
            character_style,
            text_style,
        )
        .draw(&mut display)
        .await
        .unwrap();
        display
            .fill_solid(
                &Rectangle::new(
                    Point::new(SCREEN_WIDTH as i32 / 4, SCREEN_HEIGHT as i32 * 2 / 3),
                    Size::new(8, 4),
                ),
                BinaryColor::On,
            )
            .await
            .unwrap();
        display.request_flush().await;
        Timer::after_millis(500).await;
        display.clear(BinaryColor::Off).await.unwrap();
        display.request_flush().await;
        Timer::after_millis(500).await;
    }
}

async fn line_app(mut display: CompressedDisplayPartition<DisplayType>) -> () {
    loop {
        let bb = display.bounding_box();
        // top left to bottom right
        Line::new(
            Point::new(0, 0),
            Point::new(bb.size.width as i32, bb.size.height as i32),
        )
        .draw_styled(
            &PrimitiveStyle::with_stroke(BinaryColor::On, 1),
            &mut display,
        )
        .await
        .unwrap();
        Timer::after_millis(500).await;

        // bottom left to top right
        Line::new(
            Point::new(0, bb.size.height as i32),
            Point::new(bb.size.width as i32, 0),
        )
        .draw_styled(
            &PrimitiveStyle::with_stroke(BinaryColor::On, 1),
            &mut display,
        )
        .await
        .unwrap();

        display.request_flush().await;
        // clear and loop
        Timer::after_millis(500).await;
        display.clear(BinaryColor::Off).await.unwrap();
        display.request_flush().await;
        Timer::after_millis(500).await;
    }
}

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let (display, mut window) = init_simulator_display();
    const CHUNK_HEIGHT: usize = SCREEN_HEIGHT / 2;
    let mut shared_display: SharedCompressedDisplay<CHUNK_HEIGHT, DisplayType> =
        SharedCompressedDisplay::new(display, spawner);

    let quarter_size = Size::new((SCREEN_WIDTH / 2) as u32, (SCREEN_HEIGHT / 2) as u32);
    let right_top = Rectangle::new(Point::new((SCREEN_WIDTH / 2) as i32, 0), quarter_size);
    let right_bottom = Rectangle::new(
        Point::new((SCREEN_WIDTH / 2) as i32, (SCREEN_HEIGHT / 2) as i32),
        quarter_size,
    );

    shared_display
        .launch_new_app(line_app, right_top)
        .await
        .unwrap();
    shared_display
        .launch_new_app(line_app, right_bottom)
        .await
        .unwrap();

    let left_rect = Rectangle::new(
        Point::new(0, 0),
        Size::new(SCREEN_WIDTH as u32 / 2, SCREEN_HEIGHT as u32),
    );
    shared_display
        .launch_new_app(text_app, left_rect)
        .await
        .unwrap();

    Timer::after_millis(500).await;
    shared_display
        .wait_for_flush_requests(async |d| {
            window.update(d);
            if window.events().any(|e| e == SimulatorEvent::Quit) {
                return FlushResult::Abort;
            }
            FlushResult::Continue
        })
        .await;
}
