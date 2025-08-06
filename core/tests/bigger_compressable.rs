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

const DISP_WIDTH: usize = 32;
const DISP_HEIGHT: usize = 16;
const NUM_PIXELS: usize = DISP_WIDTH * DISP_HEIGHT; // = 512

const SHOULD_PRINT_FLUSH: bool = false;
static FLUSH_REQUESTS: Channel<CriticalSectionRawMutex, u8, MAX_APPS_PER_SCREEN> = Channel::new();

#[tokio::test]
async fn split_clear() -> Result<(), NewPartitionError> {
    let buffer = [0; NUM_PIXELS];
    let size = Size::new(DISP_WIDTH as u32, DISP_HEIGHT as u32);
    let d = FakeDisplay::<NUM_PIXELS> { size, buffer };
    assert_eq!(d.buffer, [0; NUM_PIXELS]);

    let half_size = Size::new(size.width / 2, size.height);
    let left_area = Rectangle::new_at_origin(half_size);
    let right_area = Rectangle::new(Point::new(size.width as i32 / 2, 0), half_size);
    let buffers = [
        Rc::new(Mutex::new(CompressedBuffer::new(left_area.size, 0))),
        Rc::new(Mutex::new(CompressedBuffer::new(right_area.size, 0))),
    ];
    let mut left_display = CompressedDisplayPartition::<FakeDisplay<NUM_PIXELS>>::new(
        0,
        size,
        left_area,
        Rc::clone(&buffers[0]),
        &FLUSH_REQUESTS,
    )?;
    let mut right_display = CompressedDisplayPartition::<FakeDisplay<NUM_PIXELS>>::new(
        1,
        size,
        right_area,
        Rc::clone(&buffers[1]),
        &FLUSH_REQUESTS,
    )?;

    left_display.clear(BinaryColor::Off).await.unwrap();
    left_display.buffer.lock().await.check_integrity().unwrap();
    let compressed_buffer = &buffers[0].lock().await;
    let iter = DecompressingIter::new(&compressed_buffer);
    let expected = vec![0; (half_size.width * half_size.height).try_into().unwrap()];
    assert_eq!(expected, iter.collect::<Vec<u8>>());

    right_display.clear(BinaryColor::Off).await.unwrap();
    right_display.buffer.lock().await.check_integrity().unwrap();
    let compressed_buffer = &buffers[1].lock().await;
    let iter = DecompressingIter::new(&compressed_buffer);
    assert_eq!(expected, iter.collect::<Vec<u8>>());

    Ok(())
}

#[tokio::test]
async fn fill_solid() -> Result<(), NewPartitionError> {
    let buffer = [0; NUM_PIXELS];
    let size = Size::new(DISP_WIDTH as u32, DISP_HEIGHT as u32);
    let mut d = FakeDisplay::<NUM_PIXELS> { size, buffer };
    assert_eq!(*d.flush(SHOULD_PRINT_FLUSH), [0; NUM_PIXELS]);

    let half_size = Size::new(size.width / 2, size.height);
    let left_area = Rectangle::new_at_origin(half_size);
    let right_area = Rectangle::new(Point::new(size.width as i32 / 2, 0), half_size);
    let buffers = [
        Rc::new(Mutex::new(CompressedBuffer::new(left_area.size, 0))),
        Rc::new(Mutex::new(CompressedBuffer::new(right_area.size, 0))),
    ];
    let mut left_display = CompressedDisplayPartition::<FakeDisplay<NUM_PIXELS>>::new(
        0,
        size,
        left_area,
        Rc::clone(&buffers[0]),
        &FLUSH_REQUESTS,
    )?;
    let mut right_display = CompressedDisplayPartition::<FakeDisplay<NUM_PIXELS>>::new(
        1,
        size,
        right_area,
        Rc::clone(&buffers[1]),
        &FLUSH_REQUESTS,
    )?;

    let rect = Rectangle::new_at_origin(Size::new(10, 10)).translate(Point::new(2, 2));
    rect.into_styled(PrimitiveStyle::with_fill(BinaryColor::On))
        .draw(&mut left_display)
        .await
        .unwrap();
    #[rustfmt::skip]
    let expected = string_to_buffer(String::from("
        00000000 00000000
        00000000 00000000
        00111111 11110000
        00111111 11110000

        00111111 11110000
        00111111 11110000
        00111111 11110000
        00111111 11110000

        00111111 11110000
        00111111 11110000
        00111111 11110000
        00111111 11110000

        00000000 00000000
        00000000 00000000
        00000000 00000000
        00000000 00000000"));
    left_display.buffer.lock().await.check_integrity().unwrap();
    let compressed_buffer = &buffers[0].lock().await;
    let iter = DecompressingIter::new(&compressed_buffer);
    assert_eq!(expected.len(), iter.clone().count());

    let rect = Rectangle::new_at_origin(Size::new(10, 10)).translate(Point::new(6, 6));
    rect.into_styled(PrimitiveStyle::with_fill(BinaryColor::On))
        .draw(&mut right_display)
        .await
        .unwrap();
    #[rustfmt::skip]
    let expected = string_to_buffer(String::from("
        00000000 00000000
        00000000 00000000
        00000000 00000000
        00000000 00000000

        00000000 00000000
        00000000 00000000
        00000011 11111111
        00000011 11111111

        00000011 11111111
        00000011 11111111
        00000011 11111111
        00000011 11111111

        00000011 11111111
        00000011 11111111
        00000011 11111111
        00000011 11111111"));
    right_display.buffer.lock().await.check_integrity().unwrap();
    let compressed_buffer = &buffers[1].lock().await;
    let iter = DecompressingIter::new(&compressed_buffer);
    assert_eq!(expected, iter.collect::<Vec<u8>>());

    Ok(())
}

#[tokio::test]
async fn clip_outside() -> Result<(), NewPartitionError> {
    let buffer = [0; NUM_PIXELS];
    let size = Size::new(DISP_WIDTH as u32, DISP_HEIGHT as u32);
    let mut d = FakeDisplay::<NUM_PIXELS> { size, buffer };
    assert_eq!(*d.flush(SHOULD_PRINT_FLUSH), [0; NUM_PIXELS]);

    let half_size = Size::new(size.width / 2, size.height);
    let left_area = Rectangle::new_at_origin(half_size);
    let right_area = Rectangle::new(Point::new(size.width as i32 / 2, 0), half_size);
    let buffers = [
        Rc::new(Mutex::new(CompressedBuffer::new(left_area.size, 0))),
        Rc::new(Mutex::new(CompressedBuffer::new(right_area.size, 0))),
    ];
    let mut left_display = CompressedDisplayPartition::<FakeDisplay<NUM_PIXELS>>::new(
        0,
        size,
        left_area,
        Rc::clone(&buffers[0]),
        &FLUSH_REQUESTS,
    )?;
    let mut right_display = CompressedDisplayPartition::<FakeDisplay<NUM_PIXELS>>::new(
        1,
        size,
        right_area,
        Rc::clone(&buffers[1]),
        &FLUSH_REQUESTS,
    )?;

    let rect = Rectangle::new_at_origin(Size::new(10, 10)).translate(Point::new(10, 10));
    rect.into_styled(PrimitiveStyle::with_fill(BinaryColor::On))
        .draw(&mut left_display)
        .await
        .unwrap();
    #[rustfmt::skip]
    let expected = string_to_buffer(String::from("
        00000000 00000000
        00000000 00000000
        00000000 00000000
        00000000 00000000

        00000000 00000000
        00000000 00000000
        00000000 00000000
        00000000 00000000

        00000000 00000000
        00000000 00000000
        00000000 00111111
        00000000 00111111

        00000000 00111111
        00000000 00111111
        00000000 00111111
        00000000 00111111"));
    left_display.buffer.lock().await.check_integrity().unwrap();
    let compressed_buffer = &buffers[0].lock().await;
    let iter = DecompressingIter::new(&compressed_buffer);
    assert_eq!(expected, iter.collect::<Vec<u8>>());

    let rect = Rectangle::new_at_origin(Size::new(10, 10)).translate(Point::new(-6, -6));
    rect.into_styled(PrimitiveStyle::with_fill(BinaryColor::On))
        .draw(&mut right_display)
        .await
        .unwrap();
    #[rustfmt::skip]
    let expected = string_to_buffer(String::from("
        11110000 00000000
        11110000 00000000
        11110000 00000000
        11110000 00000000

        00000000 00000000
        00000000 00000000
        00000000 00000000
        00000000 00000000

        00000000 00000000
        00000000 00000000
        00000000 00000000
        00000000 00000000

        00000000 00000000
        00000000 00000000
        00000000 00000000
        00000000 00000000"));
    right_display.buffer.lock().await.check_integrity().unwrap();
    let compressed_buffer = &buffers[1].lock().await;
    let iter = DecompressingIter::new(&compressed_buffer);
    assert_eq!(expected, iter.collect::<Vec<u8>>());

    Ok(())
}
