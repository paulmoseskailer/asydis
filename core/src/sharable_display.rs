use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, channel::Channel};
use embedded_graphics::prelude::{ContainsPoint, PointsIter};
use embedded_graphics::{
    Pixel,
    draw_target::DrawTarget,
    geometry::Point,
    prelude::{Dimensions, PixelColor, Size},
    primitives::Rectangle,
};

/// Maximum number of apps allowed on the screen concurrently.
pub const MAX_APPS_PER_SCREEN: usize = 8;

/// A buffered [`DrawTarget`] that can be shared among multiple apps.
pub trait SharableBufferedDisplay: DrawTarget {
    /// The type of elements saved to the buffer - may differ from [`DrawTarget::Color`].
    type BufferElement;

    /// Specify how `Color` maps to  `BufferElement`.
    fn map_to_buffer_element(color: Self::Color) -> Self::BufferElement;

    /// Provide mutable access to the buffer.
    fn get_buffer(&mut self) -> &mut [Self::BufferElement];

    /// Calculate the buffer position of a [`Point`].
    fn calculate_buffer_index(point: Point, buffer_area_size: Size) -> usize;
}

/// Error Type for creating new screen partitions.
#[derive(Debug, PartialEq, Eq)]
pub enum NewPartitionError {
    /// Overlaps with existing partitions.
    Overlaps,
    /// Area outside the parent display.
    OutsideParent,
    /// Cannot create partitions less than 8 pixels wide.
    TooSmall,
    /// A partition should have width divisible by 8.
    BadWidth,
    /// Display width must be divisible by both pixels as well as buffer elements.
    BufferPixelMismatch,
}

/// Events from other apps that allow to alter a partition.
#[derive(Debug, PartialEq, Eq)]
pub enum AppEvent {
    /// Another app was closed
    AppClosed(Rectangle),
}

/// Things that might go wrong trying to envelope the area of an app that closed.
#[derive(Debug, PartialEq, Eq)]
pub enum EnvelopeError {
    /// The passed in AppEvent was not an AppClosed event
    WrongEvent,
    /// Enveloping the adjacent app would not result in a proper rectangle
    NotAdjacent,
    /// An error occured checking the new partition size
    PartitioningError(NewPartitionError),
}

/// A partition of a [`SharableBufferedDisplay`].
pub struct DisplayPartition<D: SharableBufferedDisplay + ?Sized> {
    id: u8,
    /// Mutable access to the entire display's buffer.
    pub buffer: *mut D::BufferElement,
    buffer_len: usize,

    /// Size of the parent display.
    pub parent_size: Size,
    /// Size of the partition itself.
    pub area: Rectangle,

    _display: core::marker::PhantomData<D>,
    flush_request_channel: &'static Channel<CriticalSectionRawMutex, u8, MAX_APPS_PER_SCREEN>,
}

impl<C, B, D> DisplayPartition<D>
where
    C: PixelColor,
    D: SharableBufferedDisplay<BufferElement = B, Color = C> + ?Sized,
{
    fn check_partition_ok(
        area: &Rectangle,
        parent_size: Size,
        buffer_len: usize,
    ) -> Result<(), NewPartitionError> {
        if area.size.width < 8 {
            return Err(NewPartitionError::TooSmall);
        }

        if Rectangle::new_at_origin(parent_size).intersection(area) != *area {
            return Err(NewPartitionError::OutsideParent);
        }

        let pixels_per_buffer_el = (parent_size.width * parent_size.height) as usize / buffer_len;
        if pixels_per_buffer_el > 0 && parent_size.width % pixels_per_buffer_el as u32 != 0 {
            return Err(NewPartitionError::BufferPixelMismatch);
        }

        if area.size.width % 8 != 0 {
            return Err(NewPartitionError::BadWidth);
        }

        Ok(())
    }

    /// Creates a new partition.
    pub fn new(
        id: u8,
        buffer: &mut [B],
        parent_size: Size,
        area: Rectangle,
        flush_request_channel: &'static Channel<CriticalSectionRawMutex, u8, MAX_APPS_PER_SCREEN>,
    ) -> Result<DisplayPartition<D>, NewPartitionError> {
        let buffer_len = buffer.len();
        Self::check_partition_ok(&area, parent_size, buffer_len)?;

        Ok(DisplayPartition {
            id,
            buffer: buffer.as_mut_ptr(),
            parent_size,
            buffer_len: buffer.len(),
            area,
            _display: core::marker::PhantomData,
            flush_request_channel,
        })
    }

    /// Request to flush this partition.
    pub async fn request_flush(&mut self) {
        self.flush_request_channel.send(self.id).await;
    }

    /// Splits the partition into two new partitions.
    pub fn split_in_two(
        &mut self,
        area1: Rectangle,
        area2: Rectangle,
    ) -> Result<(DisplayPartition<D>, DisplayPartition<D>), NewPartitionError> {
        if !area1.intersection(&area2).is_zero_sized() {
            return Err(NewPartitionError::Overlaps);
        }

        Ok((
            DisplayPartition::new(
                self.id,
                unsafe {
                    // SAFETY: self.buffer and self.buffer_len are initialized from slice in new
                    core::slice::from_raw_parts_mut(self.buffer, self.buffer_len)
                },
                self.parent_size,
                area1,
                self.flush_request_channel,
            )?,
            DisplayPartition::new(
                self.id,
                unsafe {
                    // SAFETY: self.buffer and self.buffer_len are initialized from slice in new
                    core::slice::from_raw_parts_mut(self.buffer, self.buffer_len)
                },
                self.parent_size,
                area2,
                self.flush_request_channel,
            )?,
        ))
    }

    /// Increase this partition's size from an AppClosed event.
    pub fn extend_area(&mut self, event: AppEvent) -> Result<(), EnvelopeError> {
        let other = match event {
            AppEvent::AppClosed(rect) => Ok(rect),
            //_ => Err(EnvelopeError::WrongEvent),
        }?;

        // check aligment
        let extends_above_or_below = (other.top_left.x == self.area.top_left.x)
            && (other.size.width == self.area.size.width);
        let extends_left_or_right = (other.top_left.y == self.area.top_left.y)
            && (other.size.height == self.area.size.height);

        if !(extends_above_or_below || extends_left_or_right) {
            return Err(EnvelopeError::NotAdjacent);
        }

        self.area = self.area.envelope(&other);
        Self::check_partition_ok(&self.area, self.parent_size, self.buffer_len)
            .map_err(EnvelopeError::PartitioningError)?;
        Ok(())
    }

    async fn draw_iter_internal<I>(&mut self, pixels: I) -> Result<(), D::Error>
    where
        I: ::core::iter::IntoIterator<Item = Pixel<D::Color>>,
    {
        let whole_buffer: &mut [B] =
            // Safety: we check that every index is within our owned slice
            unsafe { core::slice::from_raw_parts_mut(self.buffer, self.buffer_len) };
        for p in pixels
            .into_iter()
            .map(|pixel| Pixel(pixel.0 + self.area.top_left, pixel.1))
            .filter(|Pixel(pos, _color)| self.contains(*pos))
        {
            let buffer_index = D::calculate_buffer_index(p.0, self.parent_size);
            if self.contains(p.0) {
                whole_buffer[buffer_index] = D::map_to_buffer_element(p.1);
            }
        }
        Ok(())
    }
}

impl<D> ContainsPoint for DisplayPartition<D>
where
    D: SharableBufferedDisplay + ?Sized,
{
    fn contains(&self, p: Point) -> bool {
        self.area.contains(p)
    }
}

impl<D> Dimensions for DisplayPartition<D>
where
    D: SharableBufferedDisplay + ?Sized,
{
    fn bounding_box(&self) -> Rectangle {
        Rectangle::new_at_origin(self.area.size)
    }
}

impl<D> DrawTarget for DisplayPartition<D>
where
    D: SharableBufferedDisplay,
{
    type Color = D::Color;
    type Error = D::Error;

    async fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: ::core::iter::IntoIterator<Item = Pixel<Self::Color>>,
    {
        self.draw_iter_internal(pixels).await
    }

    async fn fill_contiguous<I>(&mut self, area: &Rectangle, colors: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Self::Color>,
    {
        let drawable_area = area.intersection(&Rectangle::new_at_origin(self.area.size));
        if drawable_area.is_zero_sized() {
            // area outside partition, noop
            return Ok(());
        }
        self.draw_iter_internal(
            drawable_area
                .points()
                .zip(colors)
                .map(|(pos, color)| Pixel(pos, color)),
        )
        .await
    }

    // Make sure to remove the offset from the Rectangle to be cleared,
    // draw_iter adds it again
    async fn clear(&mut self, color: Self::Color) -> Result<(), Self::Error> {
        self.fill_solid(&(Rectangle::new(Point::new(0, 0), self.area.size)), color)
            .await
    }
}

#[cfg(test)]
mod tests {
    use embedded_graphics::{pixelcolor::BinaryColor, prelude::OriginDimensions};

    use super::*;

    const WIDTH: u32 = 16;
    const HEIGHT: u32 = 8;
    const RESOLUTION: usize = (WIDTH * HEIGHT) as usize;
    static FLUSH_REQUESTS: Channel<CriticalSectionRawMutex, u8, MAX_APPS_PER_SCREEN> =
        Channel::new();

    struct FakeDisplay {
        buffer: [BinaryColor; RESOLUTION],
    }
    impl OriginDimensions for FakeDisplay {
        fn size(&self) -> Size {
            Size::new(WIDTH, HEIGHT)
        }
    }
    impl DrawTarget for FakeDisplay {
        type Color = BinaryColor;
        type Error = ();
        async fn draw_iter<I>(&mut self, _pixels: I) -> Result<(), Self::Error>
        where
            I: IntoIterator<Item = Pixel<Self::Color>>,
        {
            Ok(())
        }
    }
    impl SharableBufferedDisplay for FakeDisplay {
        type BufferElement = BinaryColor;
        fn map_to_buffer_element(color: Self::Color) -> Self::BufferElement {
            color
        }
        fn get_buffer(&mut self) -> &mut [Self::BufferElement] {
            &mut self.buffer
        }
        fn calculate_buffer_index(point: Point, buffer_area_size: Size) -> usize {
            point.y as usize * buffer_area_size.width as usize + point.x as usize
        }
    }
    impl core::fmt::Debug for DisplayPartition<FakeDisplay> {
        fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
            f.debug_struct("FakeDisplay")
                .field("buffer", &self.buffer)
                .finish()
        }
    }

    #[test]
    fn new_partition_error() {
        let mut display = FakeDisplay {
            buffer: [BinaryColor::Off; RESOLUTION],
        };
        let parent_size = display.bounding_box().size;
        let buffer = display.get_buffer();
        let too_small = Rectangle::new_at_origin(Size::new(7, 8));
        assert_eq!(
            DisplayPartition::new(0, buffer, parent_size, too_small, &FLUSH_REQUESTS).unwrap_err(),
            NewPartitionError::TooSmall
        );

        let too_big = Rectangle::new_at_origin(Size::new(WIDTH + 8, 8));
        assert_eq!(
            DisplayPartition::new(0, buffer, parent_size, too_big, &FLUSH_REQUESTS).unwrap_err(),
            NewPartitionError::OutsideParent
        );

        let bad_width = Rectangle::new_at_origin(Size::new(WIDTH - 1, 8));
        assert_eq!(
            DisplayPartition::new(0, buffer, parent_size, bad_width, &FLUSH_REQUESTS).unwrap_err(),
            NewPartitionError::BadWidth
        );
    }

    #[test]
    fn split_error() {
        let mut display = FakeDisplay {
            buffer: [BinaryColor::Off; RESOLUTION],
        };
        let parent_size = display.bounding_box().size;
        let buffer = display.get_buffer();

        let ok_area = Rectangle::new_at_origin(Size::new(WIDTH, HEIGHT));
        let mut partition =
            DisplayPartition::new(1, buffer, parent_size, ok_area, &FLUSH_REQUESTS).unwrap();

        let half_size = Size::new(WIDTH / 2, HEIGHT);
        let left_area = Rectangle::new_at_origin(half_size);
        let overlapping_right_area = Rectangle::new(Point::new((WIDTH / 4) as i32, 0), half_size);
        assert_eq!(
            partition
                .split_in_two(left_area, overlapping_right_area)
                .unwrap_err(),
            NewPartitionError::Overlaps
        );

        let ok_right_area = Rectangle::new(Point::new((WIDTH / 2) as i32, 0), half_size);
        partition.split_in_two(left_area, ok_right_area).unwrap();
    }
}
