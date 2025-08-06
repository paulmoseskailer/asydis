use asydis::{AppEvent, DisplayPartition, FlushResult, SharedDisplay};
use embassy_executor::Spawner;
use embassy_time::{Duration, Instant, Timer};
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

type DisplayType = SimulatorDisplay<BinaryColor>;

fn init_simulator_display() -> (DisplayType, Window) {
    let output_settings = OutputSettingsBuilder::new()
        .theme(BinaryColorTheme::OledWhite)
        .build();
    (
        SimulatorDisplay::new(Size::new(128, 64)),
        Window::new("Simulated Display", &output_settings),
    )
}

async fn text_app(mut display: DisplayPartition<DisplayType>) -> () {
    let character_style = MonoTextStyle::new(&FONT_10X20, BinaryColor::On);
    let text_style = TextStyleBuilder::new()
        .baseline(Baseline::Middle)
        .alignment(Alignment::Center)
        .build();

    let start = Instant::now();
    loop {
        Text::with_text_style(
            "hello \n world",
            Point::new(30, 20),
            character_style,
            text_style,
        )
        .draw(&mut display)
        .await
        .unwrap();
        Timer::after_millis(200).await;
        display.clear(BinaryColor::Off).await.unwrap();
        Timer::after_millis(200).await;

        if Instant::now().duration_since(start).as_secs() > 2 {
            break;
        }
    }
}

async fn line_app(mut display: DisplayPartition<DisplayType>) {
    loop {
        let max_x: i32 = (display.area.size.width - 1).try_into().unwrap();
        let max_y: i32 = (display.area.size.height - 1).try_into().unwrap();
        Line::new(Point::new(0, 0), Point::new(max_x, max_y))
            .draw_styled(
                &PrimitiveStyle::with_stroke(BinaryColor::On, 1),
                &mut display,
            )
            .await
            .unwrap();
        Timer::after_millis(200).await;
        Line::new(Point::new(0, max_y), Point::new(max_x, 0))
            .draw_styled(
                &PrimitiveStyle::with_stroke(BinaryColor::On, 1),
                &mut display,
            )
            .await
            .unwrap();
        Timer::after_millis(200).await;
        display.clear(BinaryColor::Off).await.unwrap();
        Timer::after_millis(200).await;

        match asydis::EVENTS.try_receive() {
            Err(_) => continue,
            Ok(event) => match event {
                event @ AppEvent::AppClosed(_) => display.extend_area(event).unwrap(),
            },
        };
    }
}

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let (display, mut window) = init_simulator_display();
    let mut shared_display: SharedDisplay<DisplayType> = SharedDisplay::new(display, spawner);

    let right_rect = Rectangle::new(Point::new(64, 0), Size::new(64, 64));
    shared_display
        .launch_new_app(line_app, right_rect)
        .await
        .unwrap();

    let left_rect = Rectangle::new(Point::new(0, 0), Size::new(64, 64));
    shared_display
        .launch_new_app(text_app, left_rect)
        .await
        .unwrap();

    shared_display
        .run_flush_loop_with(
            async |d, _area| {
                window.update(d);
                if window.events().any(|e| e == SimulatorEvent::Quit) {
                    return FlushResult::Abort;
                }
                FlushResult::Continue
            },
            Duration::from_millis(20),
        )
        .await;
}
