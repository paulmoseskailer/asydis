#![allow(async_fn_in_trait)]
extern crate alloc;
use alloc::boxed::Box;
use alloc::{rc::Rc, vec, vec::Vec};

use crate::{FLUSH_REQUESTS, FlushResult, NewPartitionError, SPAWNER, launch_future};
use embassy_executor::Spawner;
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, mutex::Mutex};
use embassy_time::{Duration, Instant, Timer};
use embedded_graphics::{
    geometry::{Point, Size},
    prelude::*,
    primitives::Rectangle,
};
use shared_display_core::{
    CompressableDisplay, CompressedBuffer, CompressedDisplayPartition, DecompressingIter,
    MAX_APPS_PER_SCREEN,
};

/// Shared Display with integrated RLE-compression.
///
/// Every partition holds its own RLE-buffer and implements [`DrawTarget`]. When flushing, the
/// screen is devided into chunks with CHUNK_HEIGHT, decompressing chunks one-by-one, see
/// [`SharedCompressedDisplay::run_flush_loop_with_completion`].
pub struct SharedCompressedDisplay<const CHUNK_HEIGHT: usize, D: CompressableDisplay> {
    /// The actual display, protected by a mutex.
    pub real_display: Mutex<CriticalSectionRawMutex, D>,
    size: Size,
    partition_areas: heapless::Vec<Rectangle, MAX_APPS_PER_SCREEN>,
    buffer_pointers: heapless::Vec<
        Rc<Mutex<CriticalSectionRawMutex, CompressedBuffer<D::BufferElement>>>,
        MAX_APPS_PER_SCREEN,
    >,

    spawner: &'static Spawner,
}

impl<const CHUNK_HEIGHT: usize, D: CompressableDisplay> OriginDimensions
    for SharedCompressedDisplay<CHUNK_HEIGHT, D>
{
    fn size(&self) -> Size {
        self.size
    }
}

impl<const CHUNK_HEIGHT: usize, D: CompressableDisplay> ContainsPoint
    for SharedCompressedDisplay<CHUNK_HEIGHT, D>
{
    fn contains(&self, point: Point) -> bool {
        self.bounding_box().contains(point)
    }
}

impl<const CHUNK_HEIGHT: usize, B, D> SharedCompressedDisplay<CHUNK_HEIGHT, D>
where
    B: Copy + Default + PartialEq,
    D: CompressableDisplay<BufferElement = B>,
{
    /// Creates a new Shared Compressed Display from a real display.
    pub fn new(real_display: D, spawner: Spawner) -> Self {
        let spawner_ref: &'static Spawner = SPAWNER.init(spawner);
        let size = real_display.bounding_box().size;
        assert_eq!(
            size.height as usize % CHUNK_HEIGHT,
            0,
            "chosen CHUNK_HEIGHT needs to divide screen height"
        );
        SharedCompressedDisplay {
            real_display: Mutex::new(real_display),
            size,
            partition_areas: heapless::Vec::new(),
            buffer_pointers: heapless::Vec::new(),
            spawner: spawner_ref,
        }
    }

    async fn new_partition(
        &mut self,
        area: Rectangle,
    ) -> Result<CompressedDisplayPartition<D>, NewPartitionError> {
        // check area inside display
        if !(self.contains(area.top_left)
            && self.contains(area.bottom_right().unwrap_or(area.top_left)))
        {
            return Err(NewPartitionError::OutsideParent);
        }

        // check area not overlapping with existing partition_areas
        for p in self.partition_areas.iter() {
            if p.intersection(&area).size != Size::new(0, 0) {
                return Err(NewPartitionError::Overlaps);
            }
        }
        let id: u8 = self.partition_areas.len().try_into().unwrap();
        let buffer = Rc::new(Mutex::new(CompressedBuffer::new(area.size, B::default())));
        self.buffer_pointers
            .push(buffer)
            .unwrap_or_else(|_| panic!("failed to save new partition buffer"));
        let partition = CompressedDisplayPartition::new(
            id,
            self.size,
            area,
            Rc::clone(&self.buffer_pointers[id as usize]),
            &FLUSH_REQUESTS,
        )?;

        self.partition_areas.push(area).unwrap();

        Ok(partition)
    }

    /// Launches a new app in an area of the screen.
    ///
    /// Returns an error if the area is not available, overlaps with existing apps or the screen
    /// border.
    pub async fn launch_new_app<F>(
        &mut self,
        mut app_fn: F,
        area: Rectangle,
    ) -> Result<(), NewPartitionError>
    where
        F: AsyncFnMut(CompressedDisplayPartition<D>) -> (),
        for<'b> F::CallRefFuture<'b>: 'static,
    {
        let partition = self.new_partition(area).await?;

        let fut = app_fn(partition);
        self.spawner.must_spawn(launch_future(Box::pin(fut), area));

        Ok(())
    }

    /// Runs the flush loop, additionally calling the passed in function at the end of every flush.
    ///
    /// Note that the flushing is already done internally, chunk-by-chunk, calling
    /// [`CompressableDisplay::flush_chunk`] for every decompressed chunk. The passed in function can be used to
    /// complete a flush, for example if [`CompressableDisplay::flush_chunk`] draws to a buffer
    /// that has to be drawn to the actual screen. It is called once per flush, after all chunks have been
    /// decompressed.
    /// Only exits if the flush function returns [`FlushResult::Abort`].
    pub async fn run_flush_loop_with_completion<F>(
        &self,
        mut flush_complete_fn: F,
        flush_interval: Duration,
    ) where
        F: AsyncFnMut(&mut D) -> FlushResult,
    {
        loop {
            if self.partition_areas.is_empty() {
                Timer::after(flush_interval).await;
                continue;
            }

            let num_chunks = self.size.height as usize / CHUNK_HEIGHT;
            for chunk in 0..num_chunks {
                let start = Instant::now().as_micros();
                let chunk_area = Rectangle::new(
                    Point::new(0, (chunk * CHUNK_HEIGHT) as i32),
                    Size::new(self.size.width, CHUNK_HEIGHT as u32),
                );

                let decompressed_chunk: Vec<D::BufferElement> =
                    self.decompress_chunk(chunk_area).await;
                self.real_display
                    .lock()
                    .await
                    .flush_chunk(&decompressed_chunk, chunk_area)
                    .await;
                let end = Instant::now().as_micros();
                defmt::info!(
                    "APP 5 FLUSH {}ms ({} - {})",
                    (end.saturating_sub(start)) / 1000,
                    start / 1000,
                    end / 1000
                );
            }

            let flush_result = flush_complete_fn(&mut *self.real_display.lock().await).await;
            match flush_result {
                FlushResult::Continue => {}
                FlushResult::Abort => {
                    break;
                }
            }

            Timer::after(flush_interval).await;
        }
    }

    /// Spawns a background task that waits for flush requests from all [`CompressedDisplayPartition`]s and flushes.
    pub async fn wait_for_flush_requests<F>(
        &self,
        mut flush_complete_fn: F,
        retry_interval: Duration,
    ) where
        F: AsyncFnMut(&mut D) -> FlushResult,
    {
        loop {
            match FLUSH_REQUESTS.try_receive() {
                Err(_) => {
                    Timer::after(retry_interval).await;
                }
                Ok(_) => {
                    // cannot flush a single partition, just flush all
                    let num_chunks = self.size.height as usize / CHUNK_HEIGHT;
                    for chunk in 0..num_chunks {
                        let start = Instant::now().as_micros();
                        let chunk_area = Rectangle::new(
                            Point::new(0, (chunk * CHUNK_HEIGHT) as i32),
                            Size::new(self.size.width, CHUNK_HEIGHT as u32),
                        );

                        let decompressed_chunk: Vec<D::BufferElement> =
                            self.decompress_chunk(chunk_area).await;
                        self.real_display
                            .lock()
                            .await
                            .flush_chunk(&decompressed_chunk, chunk_area)
                            .await;
                        let end = Instant::now().as_micros();
                        defmt::info!(
                            "APP 5 FLUSH {}ms ({} - {})",
                            (end.saturating_sub(start)) / 1000,
                            start / 1000,
                            end / 1000
                        );
                    }

                    let flush_result =
                        flush_complete_fn(&mut *self.real_display.lock().await).await;
                    match flush_result {
                        FlushResult::Continue => {}
                        FlushResult::Abort => {
                            break;
                        }
                    }
                }
            }
        }
    }

    async fn decompress_chunk(&self, chunk_area: Rectangle) -> Vec<D::BufferElement> {
        let resolution = chunk_area.size.width * chunk_area.size.height;
        assert_eq!(
            chunk_area.top_left.x, 0,
            "a chunk does not span the entire width of the screen"
        );
        assert_eq!(
            chunk_area.size.width, self.size.width,
            "a chunk does not span the entire width of the screen"
        );

        let mut decompressed_chunk: Vec<D::BufferElement> =
            vec![D::BufferElement::default(); resolution as usize];
        for (i, partition_area) in self.partition_areas.iter().enumerate() {
            let intersection: Rectangle = partition_area.intersection(&chunk_area);
            if intersection.size == Size::zero() {
                continue;
            }

            // decompress intersection with partition
            let compressed_partition = &self.buffer_pointers[i].lock().await;

            // copy decompressed intersection into chunk row by row
            let y_offset_in_chunk = (intersection.top_left.y - chunk_area.top_left.y) as usize;
            let x_offset_in_chunk = intersection.top_left.x as usize; //chunk starts at x=0
            let start_index_in_chunk =
                y_offset_in_chunk * chunk_area.size.width as usize + x_offset_in_chunk;

            let y_offset_in_partition =
                (intersection.top_left.y - partition_area.top_left.y) as usize;
            let x_offset_in_partition =
                (intersection.top_left.x - partition_area.top_left.x) as usize;
            let start_index_in_partition =
                y_offset_in_partition * intersection.size.width as usize + x_offset_in_partition;
            let mut partition_iter =
                DecompressingIter::new(compressed_partition).skip(start_index_in_partition);

            let pixels_to_copy_per_row = intersection.size.width as usize;

            for row in 0..(intersection.size.height as usize) {
                let row_start_index_chunk =
                    start_index_in_chunk + (chunk_area.size.width as usize) * row;
                if row_start_index_chunk + pixels_to_copy_per_row > decompressed_chunk.len() {
                    panic!("destination buffer index out of range");
                }

                for (dst, src) in decompressed_chunk
                    [row_start_index_chunk..(row_start_index_chunk + pixels_to_copy_per_row)]
                    .iter_mut()
                    .zip(partition_iter.by_ref().take(pixels_to_copy_per_row))
                {
                    *dst = src;
                }
            }
        }
        decompressed_chunk
    }
}
