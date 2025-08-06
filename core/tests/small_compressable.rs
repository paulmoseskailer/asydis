use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, channel::Channel, mutex::Mutex};
use embedded_graphics::{
    draw_target::DrawTarget,
    geometry::Point,
    pixelcolor::BinaryColor,
    prelude::*,
    primitives::{PrimitiveStyle, Rectangle},
};
use shared_display_core::{
    CompressedBuffer, CompressedDisplayPartition, DecompressingIter, MAX_APPS_PER_SCREEN,
    NewPartitionError,
};
extern crate alloc;
use alloc::rc::Rc;

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
    let size = Size::new(DISP_WIDTH as u32, DISP_HEIGHT as u32);
    let mut d = FakeDisplay::<NUM_PIXELS> { size, buffer };
    assert_eq!(*d.flush(SHOULD_PRINT_FLUSH), [0; NUM_PIXELS]);

    d.clear(BinaryColor::On).await.unwrap();
    assert_eq!(*d.flush(SHOULD_PRINT_FLUSH), [1; NUM_PIXELS]);

    let parent_size = d.size();
    let left_area = Rectangle::new(Point::new(0, 0), Size::new(8, 2));
    let right_area = Rectangle::new(Point::new(8, 0), Size::new(8, 2));
    let buffers = [
        Rc::new(Mutex::new(CompressedBuffer::new(left_area.size, 0))),
        Rc::new(Mutex::new(CompressedBuffer::new(right_area.size, 0))),
    ];
    let mut left_display = CompressedDisplayPartition::<FakeDisplay<NUM_PIXELS>>::new(
        0,
        parent_size,
        left_area,
        Rc::clone(&buffers[0]),
        &FLUSH_REQUESTS,
    )?;
    let mut right_display = CompressedDisplayPartition::<FakeDisplay<NUM_PIXELS>>::new(
        1,
        parent_size,
        right_area,
        Rc::clone(&buffers[1]),
        &FLUSH_REQUESTS,
    )?;

    left_display.clear(BinaryColor::Off).await.unwrap();
    left_display.buffer.lock().await.check_integrity().unwrap();
    let compressed_buffer = &buffers[0].lock().await;
    let iter = DecompressingIter::new(&compressed_buffer);
    let expected = string_to_buffer(String::from("00000000 00000000"));
    assert_eq!(expected, iter.collect::<Vec<u8>>());

    right_display.clear(BinaryColor::Off).await.unwrap();
    right_display.buffer.lock().await.check_integrity().unwrap();
    let compressed_buffer = &buffers[1].lock().await;
    let iter = DecompressingIter::new(&compressed_buffer);
    assert_eq!(expected, iter.collect::<Vec<u8>>());

    Ok(())
}

#[tokio::test]
async fn simple_split_draw_iter() -> Result<(), NewPartitionError> {
    let buffer = [0; NUM_PIXELS];
    let size = Size::new(DISP_WIDTH as u32, DISP_HEIGHT as u32);
    let mut d = FakeDisplay::<NUM_PIXELS> { size, buffer };
    assert_eq!(*d.flush(SHOULD_PRINT_FLUSH), [0; NUM_PIXELS]);

    let parent_size = d.size();
    let left_area = Rectangle::new(Point::new(0, 0), Size::new(8, 2));
    let right_area = Rectangle::new(Point::new(8, 0), Size::new(8, 2));
    let buffers = [
        Rc::new(Mutex::new(CompressedBuffer::new(left_area.size, 0))),
        Rc::new(Mutex::new(CompressedBuffer::new(right_area.size, 0))),
    ];
    let mut left_display = CompressedDisplayPartition::<FakeDisplay<NUM_PIXELS>>::new(
        0,
        parent_size,
        left_area,
        Rc::clone(&buffers[0]),
        &FLUSH_REQUESTS,
    )?;
    let mut right_display = CompressedDisplayPartition::<FakeDisplay<NUM_PIXELS>>::new(
        1,
        parent_size,
        right_area,
        Rc::clone(&buffers[1]),
        &FLUSH_REQUESTS,
    )?;

    let rect = Rectangle::new(Point::new(0, 0), Size::new(2, 2));
    rect.into_styled(PrimitiveStyle::with_stroke(BinaryColor::On, 1))
        .draw(&mut right_display)
        .await
        .unwrap();
    let expected = string_to_buffer(String::from("11000000 11000000"));
    right_display.buffer.lock().await.check_integrity().unwrap();
    let compressed_buffer = &buffers[1].lock().await;
    let iter = DecompressingIter::new(&compressed_buffer);
    assert_eq!(expected, iter.collect::<Vec<u8>>());

    let rect = Rectangle::new(Point::new(0, 0), Size::new(5, 2));
    rect.into_styled(PrimitiveStyle::with_stroke(BinaryColor::On, 1))
        .draw(&mut left_display)
        .await
        .unwrap();
    let expected = string_to_buffer(String::from("11111000 11111000 "));
    left_display.buffer.lock().await.check_integrity().unwrap();
    let compressed_buffer = &buffers[0].lock().await;
    let iter = DecompressingIter::new(&compressed_buffer);
    assert_eq!(expected, iter.collect::<Vec<u8>>());

    Ok(())
}
