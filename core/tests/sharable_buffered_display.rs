use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, channel::Channel};
use embedded_graphics::{
    draw_target::DrawTarget,
    geometry::Point,
    pixelcolor::BinaryColor,
    prelude::*,
    primitives::{PrimitiveStyle, Rectangle},
};
use shared_display_core::{
    DisplayPartition, MAX_APPS_PER_SCREEN, NewPartitionError, SharableBufferedDisplay,
};
mod common;
use crate::common::*;

const DISP_WIDTH: usize = 16;
const DISP_HEIGHT: usize = 2;
const NUM_PIXELS: usize = DISP_WIDTH * DISP_HEIGHT;

const SHOULD_PRINT_FLUSH: bool = false;
static FLUSH_REQUESTS: Channel<CriticalSectionRawMutex, u8, MAX_APPS_PER_SCREEN> = Channel::new();

#[tokio::test]
async fn simple_split_clear() -> Result<(), NewPartitionError> {
    let buffer = [0; NUM_PIXELS];
    let mut d = FakeDisplay::<NUM_PIXELS> {
        size: Size::new(DISP_WIDTH as u32, DISP_HEIGHT as u32),
        buffer,
    };
    assert_eq!(*d.flush(SHOULD_PRINT_FLUSH), [0; NUM_PIXELS]);

    d.clear(BinaryColor::On).await.unwrap();
    assert_eq!(*d.flush(SHOULD_PRINT_FLUSH), [1; NUM_PIXELS]);

    let parent_size = d.size();
    let left_area = Rectangle::new(Point::new(0, 0), Size::new(8, 2));
    let mut left_display = DisplayPartition::<FakeDisplay<NUM_PIXELS>>::new(
        0,
        d.get_buffer(),
        parent_size,
        left_area,
        &FLUSH_REQUESTS,
    )?;
    let right_area = Rectangle::new(Point::new(8, 0), Size::new(8, 2));
    let mut right_display = DisplayPartition::<FakeDisplay<NUM_PIXELS>>::new(
        1,
        d.get_buffer(),
        parent_size,
        right_area,
        &FLUSH_REQUESTS,
    )?;

    left_display.clear(BinaryColor::Off).await.unwrap();
    let expected = string_to_buffer(String::from("00000000 11111111 00000000 11111111"));
    assert_eq!(expected, *d.flush(SHOULD_PRINT_FLUSH));

    d.clear(BinaryColor::On).await.unwrap();
    assert_eq!(*d.flush(SHOULD_PRINT_FLUSH), [1; NUM_PIXELS]);

    right_display.clear(BinaryColor::Off).await.unwrap();
    let expected = string_to_buffer(String::from("11111111 00000000 11111111 00000000"));
    assert_eq!(expected, *d.flush(SHOULD_PRINT_FLUSH));

    Ok(())
}

#[tokio::test]
async fn simple_split_draw_iter() -> Result<(), NewPartitionError> {
    let buffer = [0; NUM_PIXELS];
    let mut d = FakeDisplay::<NUM_PIXELS> {
        size: Size::new(DISP_WIDTH as u32, DISP_HEIGHT as u32),
        buffer,
    };
    assert_eq!(*d.flush(SHOULD_PRINT_FLUSH), [0; NUM_PIXELS]);

    let parent_size = d.size();
    let left_area = Rectangle::new(Point::new(0, 0), Size::new(8, 2));
    let mut left_display = DisplayPartition::<FakeDisplay<NUM_PIXELS>>::new(
        0,
        d.get_buffer(),
        parent_size,
        left_area,
        &FLUSH_REQUESTS,
    )?;
    let right_area = Rectangle::new(Point::new(8, 0), Size::new(8, 2));
    let mut right_display = DisplayPartition::<FakeDisplay<NUM_PIXELS>>::new(
        1,
        d.get_buffer(),
        parent_size,
        right_area,
        &FLUSH_REQUESTS,
    )?;

    let rect = Rectangle::new(Point::new(0, 0), Size::new(2, 2));
    rect.into_styled(PrimitiveStyle::with_stroke(BinaryColor::On, 1))
        .draw(&mut right_display)
        .await
        .unwrap();
    let expected = string_to_buffer(String::from("00000000 11000000 00000000 11000000"));
    assert_eq!(expected, *d.flush(SHOULD_PRINT_FLUSH));

    let rect = Rectangle::new(Point::new(0, 0), Size::new(5, 2));
    rect.into_styled(PrimitiveStyle::with_stroke(BinaryColor::On, 1))
        .draw(&mut left_display)
        .await
        .unwrap();
    let expected = string_to_buffer(String::from("11111000 11000000 11111000 11000000"));
    assert_eq!(expected, *d.flush(SHOULD_PRINT_FLUSH));

    Ok(())
}
