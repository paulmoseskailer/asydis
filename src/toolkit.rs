#![allow(async_fn_in_trait)]
extern crate alloc;
use alloc::boxed::Box;

use ::core::{future::Future, pin::Pin};
use embassy_executor::Spawner;
use embassy_sync::{
    blocking_mutex::raw::CriticalSectionRawMutex, channel::Channel, mutex::Mutex, signal::Signal,
};
use embassy_time::{Duration, Timer};
use embedded_graphics::{geometry::Size, primitives::Rectangle};
use static_cell::StaticCell;

use shared_display_core::{
    AppEvent, AreaToFlush, DisplayPartition, DrawTracker, MAX_APPS_PER_SCREEN, NewPartitionError,
    SharableBufferedDisplay,
};

const EVENT_QUEUE_SIZE: usize = MAX_APPS_PER_SCREEN;
pub(crate) static SPAWNER: StaticCell<Spawner> = StaticCell::new();
static DRAW_TRACKERS: [DrawTracker; MAX_APPS_PER_SCREEN] =
    [const { DrawTracker::new() }; MAX_APPS_PER_SCREEN];

/// Interval between to flushes.
pub const FLUSH_INTERVAL: Duration = Duration::from_millis(20);
/// Event queue for all apps to access.
pub static EVENTS: Channel<CriticalSectionRawMutex, AppEvent, EVENT_QUEUE_SIZE> = Channel::new();
/// Signals for partitions to request flushing
static FLUSH_REQUESTS: [Signal<CriticalSectionRawMutex, ()>; MAX_APPS_PER_SCREEN] =
    [const { Signal::new() }; MAX_APPS_PER_SCREEN];

/// Whether to continue flushing or not.
#[derive(PartialEq, Eq)]
pub enum FlushResult {
    /// Continue flushing
    Continue,
    /// Abort the loop (e.g. when the simulator window was closed)
    Abort,
}

/// Shared Display.
pub struct SharedDisplay<D: SharableBufferedDisplay> {
    /// The actual display, locked with mutex
    pub real_display: Mutex<CriticalSectionRawMutex, D>,
    partition_areas: heapless::Vec<Rectangle, MAX_APPS_PER_SCREEN>,
    draw_trackers: &'static [DrawTracker; MAX_APPS_PER_SCREEN],

    spawner: &'static Spawner,
}

impl<B, D> SharedDisplay<D>
where
    D: SharableBufferedDisplay<BufferElement = B>,
{
    /// Creates a new Shared Display from a real display.
    pub fn new(real_display: D, spawner: Spawner) -> Self {
        let spawner_ref: &'static Spawner = SPAWNER.init(spawner);
        SharedDisplay {
            real_display: Mutex::new(real_display),
            partition_areas: heapless::Vec::new(),
            draw_trackers: &DRAW_TRACKERS,
            spawner: spawner_ref,
        }
    }

    async fn new_partition(
        &mut self,
        area: Rectangle,
    ) -> Result<DisplayPartition<D>, NewPartitionError> {
        let real_display: &mut D = &mut *self.real_display.lock().await;

        // check area inside display
        let bb = real_display.bounding_box();
        if !(bb.contains(area.top_left)
            && bb.contains(area.bottom_right().unwrap_or(area.top_left)))
        {
            return Err(NewPartitionError::OutsideParent);
        }

        // check area not overlapping with existing partition_areas
        for p in self.partition_areas.iter() {
            if p.intersection(&area).size != Size::new(0, 0) {
                return Err(NewPartitionError::Overlaps);
            }
        }

        let index = self.partition_areas.len();
        let result =
            real_display.new_partition(area, &self.draw_trackers[index], &FLUSH_REQUESTS[index]);

        if result.is_ok() {
            self.partition_areas.push(area).unwrap();
        }

        result
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
        F: AsyncFnMut(DisplayPartition<D>),
        for<'b> F::CallRefFuture<'b>: 'static,
    {
        let partition = self.new_partition(area).await?;

        let fut = app_fn(partition);
        self.spawner.must_spawn(launch_future(Box::pin(fut), area));

        Ok(())
    }

    /// Launches a new app that can launch other apps in an area of the screen.
    ///
    /// See [`launch_app_in_app`].
    /// Returns an error if the area is not available, overlaps with existing apps or the screen
    /// border.
    pub async fn launch_new_recursive_app<F>(
        &mut self,
        mut app_fn: F,
        area: Rectangle,
    ) -> Result<(), NewPartitionError>
    where
        F: AsyncFnMut(DisplayPartition<D>, &'static Spawner) -> (),
        for<'b> F::CallRefFuture<'b>: 'static,
    {
        let partition = self.new_partition(area).await?;

        let fut = app_fn(partition, self.spawner);
        self.spawner.must_spawn(launch_future(Box::pin(fut), area));

        Ok(())
    }

    /// Runs a given flush function in a loop.
    ///
    /// Provides the passed in function with a Rectangle of the area that has been drawn to since
    /// the last flush.
    /// Only exits if the flush function returns [`FlushResult::Abort`].
    pub async fn run_flush_loop_with<F>(&self, mut flush_area_fn: F)
    where
        F: AsyncFnMut(&mut D, Rectangle) -> FlushResult,
    {
        'outer: loop {
            for (index, draw_tracker) in self.draw_trackers.iter().enumerate() {
                let area_to_flush = match draw_tracker.take_dirty_area().await {
                    AreaToFlush::None => continue,
                    AreaToFlush::All => self.partition_areas[index],
                    AreaToFlush::Some(rect) => {
                        let offset = self.partition_areas[index].top_left;
                        Rectangle::new(rect.top_left + offset, rect.size)
                    }
                };
                let flush_result =
                    flush_area_fn(&mut *self.real_display.lock().await, area_to_flush).await;
                if flush_result == FlushResult::Abort {
                    break 'outer;
                }
            }
            Timer::after(FLUSH_INTERVAL).await;
        }
    }

    /// Spawns a background task that waits for flush requests from [`DisplayPartition`]s and flushes
    pub async fn wait_for_flush_requests<F>(&self, mut flush_area_fn: F)
    where
        F: AsyncFnMut(&mut D, Rectangle) -> FlushResult,
    {
        loop {
            for (i, signal) in FLUSH_REQUESTS.iter().enumerate() {
                if let Some(_) = signal.try_take() {
                    let flush_result = flush_area_fn(
                        &mut *self.real_display.lock().await,
                        self.partition_areas[i],
                    )
                    .await;
                    if flush_result == FlushResult::Abort {
                        break;
                    }
                }
            }
            Timer::after(FLUSH_INTERVAL).await;
        }
    }
}

#[embassy_executor::task(pool_size = MAX_APPS_PER_SCREEN)]
pub(crate) async fn launch_future(app_future: Pin<Box<dyn Future<Output = ()>>>, area: Rectangle) {
    app_future.await;

    EVENTS.send(AppEvent::AppClosed(area)).await;
}

/// Launches an app from inside another app.
pub async fn launch_app_in_app<F, D>(
    spawner: &'static Spawner,
    mut app_fn: F,
    partition: DisplayPartition<D>,
) where
    D: SharableBufferedDisplay,
    F: AsyncFnMut(DisplayPartition<D>) -> (),
    for<'b> F::CallRefFuture<'b>: 'static,
{
    let area = partition.area;
    let fut = app_fn(partition);
    spawner.must_spawn(launch_future(Box::pin(fut), area));
}
