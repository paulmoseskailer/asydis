use core::convert::Infallible;
use embedded_graphics::{
    Pixel, draw_target::DrawTarget, geometry::Point, pixelcolor::BinaryColor, prelude::*,
    primitives::Rectangle,
};
use shared_display_core::{CompressableDisplay, SharableBufferedDisplay};

pub fn string_to_buffer(s: String) -> Vec<u8> {
    s.chars()
        .filter(|&c| c == '0' || c == '1')
        .map(|c| match c {
            '0' => 0,
            '1' => 1,
            _ => panic!(),
        })
        .collect()
}
pub struct FakeDisplay<const NUM_PIXELS: usize> {
    pub size: Size,
    pub buffer: [u8; NUM_PIXELS],
}

impl<const NUM_PIXELS: usize> FakeDisplay<NUM_PIXELS> {
    pub fn flush(&mut self, should_print: bool) -> &[u8; NUM_PIXELS] {
        if should_print {
            for row in 0..self.size.height {
                let row_start: usize = (row * self.size.width).try_into().unwrap();
                for i in 0..self.size.width {
                    print!("{}", self.buffer[row_start + i as usize]);
                }
                println!("");
            }
        }
        &self.buffer
    }
}

impl<const NUM_PIXELS: usize> OriginDimensions for FakeDisplay<NUM_PIXELS> {
    fn size(&self) -> Size {
        self.size
    }
}

impl<const NUM_PIXELS: usize> DrawTarget for FakeDisplay<NUM_PIXELS> {
    type Color = BinaryColor;
    type Error = Infallible;

    async fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Self::Color>>,
    {
        pixels.into_iter().for_each(|Pixel(pos, color)| {
            assert!(pos.x < self.size.width as i32);
            let pixel_index: usize = (pos.y * self.size.width as i32 + pos.x).try_into().unwrap();
            assert!(pixel_index < NUM_PIXELS);
            self.buffer[pixel_index] = match color {
                BinaryColor::On => 1,
                BinaryColor::Off => 0,
            };
        });
        Ok(())
    }
}

impl<const NUM_PIXELS: usize> SharableBufferedDisplay for FakeDisplay<NUM_PIXELS> {
    type BufferElement = u8;
    fn get_buffer(&mut self) -> &mut [Self::BufferElement] {
        self.buffer.as_mut()
    }

    fn calculate_buffer_index(point: Point, parent_size: Size) -> usize {
        (point.y * parent_size.width as i32 + point.x)
            .try_into()
            .unwrap()
    }
    fn map_to_buffer_element(color: Self::Color) -> Self::BufferElement {
        match color {
            BinaryColor::On => 1,
            BinaryColor::Off => 0,
        }
    }
}

impl<const NUM_PIXELS: usize> CompressableDisplay for FakeDisplay<NUM_PIXELS> {
    type BufferElement = u8;
    fn calculate_buffer_index(point: Point, parent_size: Size) -> usize {
        (point.y * parent_size.width as i32 + point.x)
            .try_into()
            .unwrap()
    }
    fn map_to_buffer_element(color: Self::Color) -> Self::BufferElement {
        match color {
            BinaryColor::On => 1,
            BinaryColor::Off => 0,
        }
    }
    async fn flush_chunk(&mut self, chunk: &[Self::BufferElement], chunk_area: Rectangle) {
        for (i, p) in chunk_area.points().enumerate() {
            self.buffer[<FakeDisplay<NUM_PIXELS> as CompressableDisplay>::calculate_buffer_index(
                p,
                chunk_area.size,
            )] = chunk[i];
        }
    }
}
