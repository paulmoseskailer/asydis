use core::cmp::PartialEq;
use embedded_graphics::prelude::*;

// requires embedded-alloc for no_std
extern crate alloc;
use alloc::{vec, vec::Vec};

/// An RLE-encoded framebuffer.
#[allow(clippy::box_collection)]
#[derive(Clone)]
pub struct CompressedBuffer<B: Copy + PartialEq> {
    pub(crate) inner: Vec<(B, u8)>,
    decompressed_size: Size,
}

impl<B: Copy + PartialEq> CompressedBuffer<B> {
    /// Creates a new compressed buffer with a start value.
    pub fn new(decompressed_size: Size, start_value: B) -> Self {
        let num_pixels = decompressed_size.width * decompressed_size.height;
        let full_runs = num_pixels / 255;
        let mut buffer = vec![(start_value, 255); full_runs as usize];
        let remainder = num_pixels - (full_runs * 255);
        if remainder > 0 {
            buffer.push((start_value, remainder.try_into().unwrap()));
        }
        Self {
            inner: buffer,
            decompressed_size,
        }
    }

    /// Checks whether the buffer still encodes as many elements as it should.
    pub fn check_integrity(&self) -> Result<(), ()> {
        self.inner.iter().for_each(|&(_color, run_len)| {
            assert_ne!(run_len, 0, "found run with length 0");
        });
        let decompressed_buffer_len = self.decompressed_size.width * self.decompressed_size.height;
        let actual_len = self
            .inner
            .iter()
            .fold(0_u64, |before, (_color, run_len)| before + *run_len as u64);
        if actual_len == decompressed_buffer_len as u64 {
            return Ok(());
        }
        Err(())
    }

    // merges any two consecutive runs with the same color that would fit into a single run
    fn merge_remainders(&mut self) {
        let mut optimal_until = 0;
        while optimal_until < (self.inner.len() - 1) {
            'inner: for run in optimal_until..(self.inner.len() - 1) {
                let (color, len) = self.inner[run];
                let (next_color, next_len) = self.inner[run + 1];
                if color != next_color || len as usize + next_len as usize > 255 {
                    // this is fine
                    optimal_until = run + 1;
                    continue 'inner;
                } else {
                    // these should be merged into one
                    self.inner[run].1 = len.saturating_add(next_len);
                    self.inner.remove(run + 1);
                    break 'inner;
                }
            }
        }

        // ensure no more merging possible
        self.inner.chunks_exact(2).for_each(|chunk| {
            let (b1, len_1) = chunk[0];
            let (b2, len_2) = chunk[1];
            if (b1 == b2) && (len_1.saturating_add(len_2)) < 255 {
                panic!("not optimal");
            }
        });
    }

    // Finds the run that contains the decompressed target_index.
    // Returns run_index and decompressed start index for that run.
    fn find_run_with_index(&self, target_index: usize) -> Option<(usize, usize)> {
        let mut current_index = 0;
        let mut run_index = 0;
        for (_color, run_length) in self.inner.iter() {
            if current_index + *run_length as usize > target_index {
                break;
            }
            current_index += *run_length as usize;
            run_index += 1;
        }

        if run_index == self.inner.len() {
            None
        } else {
            Some((run_index, current_index))
        }
    }

    pub(crate) fn set_at_index(&mut self, target_index: usize, new_value: B) -> Result<(), ()> {
        let (run_index, decompressed_run_start) =
            self.find_run_with_index(target_index).ok_or(())?;

        let (buffer_value_previously, run_len_previously) = &self.inner[run_index];
        if new_value == *buffer_value_previously {
            // nothing to do, color already set
            return Ok(());
        }
        let (buffer_previously, run_len_previously) =
            (*buffer_value_previously, *run_len_previously);

        let run_before_len = target_index - decompressed_run_start;
        let run_after_len =
            (decompressed_run_start + run_len_previously as usize) - (target_index + 1);

        let have_run_before = run_before_len > 0;
        let have_run_after = run_after_len > 0;

        // Check if we can merge with previous run
        if !have_run_before && run_index > 0 {
            let (color_before, run_len_before) = &self.inner[run_index - 1];
            if *color_before == new_value && *run_len_before < 255 {
                // add current pixel to previous run
                self.inner[run_index - 1].1 += 1;
                self.inner[run_index].1 -= 1;
                if self.inner[run_index].1 == 0 {
                    // remove run
                    self.inner.remove(run_index);
                    // possibly merge run after
                    if run_index < self.inner.len() {
                        let (color_after, run_len_after) = &self.inner[run_index];
                        let combined_len =
                            self.inner[run_index - 1].1.saturating_add(*run_len_after);
                        if combined_len < 255 && *color_after == new_value {
                            self.inner[run_index - 1].1 = combined_len;
                            self.inner.remove(run_index);
                        }
                    }
                }
                // Merged before, possibly after, done
                return Ok(());
            }
        }

        // check if we can merge with next run (even if we can't merge with previous)
        if !have_run_after && run_index < (self.inner.len() - 1) {
            let (color_after, run_len_after) = &self.inner[run_index + 1];
            if *color_after == new_value && *run_len_after < 255 {
                self.inner[run_index + 1].1 += 1;
                self.inner[run_index].1 -= 1;
                if self.inner[run_index].1 == 0 {
                    self.inner.remove(run_index);
                }
                // Merged with next run, done
                return Ok(());
            }
        }

        // new pixel
        self.inner[run_index] = (new_value, 1);
        if have_run_before {
            self.inner.insert(
                run_index,
                (buffer_previously, run_before_len.try_into().unwrap()),
            );
        }
        if run_after_len > 0 {
            let index = run_index + 1 + have_run_before as usize;
            self.inner.insert(
                index,
                (buffer_previously, run_after_len.try_into().unwrap()),
            );
        }

        self.merge_remainders();
        if self.check_integrity().is_err() {
            panic!(
                "after set_at_index({}) check_integrity failed",
                target_index
            );
        }

        Ok(())
    }

    pub(crate) fn set_at_index_contiguous(
        &mut self,
        target_index: usize,
        new_value: B,
        mut num_elements: usize,
    ) -> Result<(), ()> {
        let end_index = target_index + num_elements;
        let (mut run_index, mut decompressed_run_start) =
            self.find_run_with_index(target_index).ok_or(())?;
        let (mut color_before, mut run_len) = self.inner[run_index];
        let next_run_start = decompressed_run_start + run_len as usize;
        let mut elements_left_in_run: u8 = u8::try_from(next_run_start - target_index).unwrap();

        // check if this run already has the correct color
        while color_before == new_value {
            // skip to next run
            run_index += 1;
            decompressed_run_start += run_len as usize;
            num_elements = num_elements.saturating_sub(elements_left_in_run as usize);
            (color_before, run_len) = self.inner[run_index];
            elements_left_in_run = run_len;

            if num_elements == 0 {
                return Ok(());
            }
        }
        assert!(
            decompressed_run_start <= end_index,
            "set_at_index_contiguous skipped too many runs!"
        );

        let insert_index = self.remove_n_elements_starting_at_run_x_index_i(
            num_elements,
            run_index,
            run_len.saturating_sub(elements_left_in_run),
        );
        self.add_n_elements_at_run_x(num_elements, new_value, insert_index);
        self.merge_remainders();
        self.check_integrity()?;

        Ok(())
    }

    // Removes n elements starting at the ith element of run x.
    // Returns the index of the run in which to insert new elements in place of the removed ones.
    fn remove_n_elements_starting_at_run_x_index_i(
        &mut self,
        mut elements_to_remove: usize,
        run_index: usize,
        inside_run_index: u8,
    ) -> usize {
        if elements_to_remove == 0 {
            todo!();
        }

        // 1. Possibly split off the beginning of run x
        let (run_x_color, run_x_len) = self.inner[run_index];
        assert!(inside_run_index < run_x_len);
        if inside_run_index > 0 {
            // split run in two
            // left part
            self.inner[run_index].1 = inside_run_index;
            // right part
            let right_split_len = run_x_len - inside_run_index;
            self.inner
                .insert(run_index + 1, (run_x_color, right_split_len));
        }

        // where to insert new block after the removal
        let insert_index_afterwards = match inside_run_index {
            // no split took place, insert/delete at run x
            0 => run_index,
            // first run was split off, insert/delete right after
            _ => run_index + 1,
        };

        // 2. Remove remaining elements
        while elements_to_remove > 0 {
            let (_color, next_run_len) = self.inner[insert_index_afterwards];
            let keep_next_run = elements_to_remove < next_run_len as usize;
            if keep_next_run {
                // only shorten the run, don't remove entirely
                self.inner[insert_index_afterwards].1 -= u8::try_from(elements_to_remove).unwrap();
            } else {
                // need to remove at least as many elements as the run is long
                // therefore delete the entire run
                self.inner.remove(insert_index_afterwards);
            }
            elements_to_remove = elements_to_remove.saturating_sub(next_run_len as usize);
        }

        insert_index_afterwards
    }

    fn add_n_elements_at_run_x(&mut self, num_elements: usize, new_value: B, run_index: usize) {
        let full_runs = num_elements / 255;
        for _ in 0..full_runs {
            self.inner.insert(run_index, (new_value, 255));
        }
        let remainder = num_elements - (full_runs * 255);
        if remainder > 0 {
            self.inner
                .insert(run_index, (new_value, remainder.try_into().unwrap()));
        }
    }

    /// Empties the buffer and refill it with a new value.
    pub fn clear_and_refill(&mut self, new_value: B) {
        // empty first
        self.inner.clear();
        // then re-fill
        let num_pixels = self.decompressed_size.width * self.decompressed_size.height;
        let full_runs = num_pixels / 255;
        for _ in 0..full_runs {
            self.inner.push((new_value, 255));
        }
        let remainder = num_pixels - (full_runs * 255);
        if remainder > 0 {
            self.inner.push((new_value, remainder.try_into().unwrap()));
        }
    }
}

/// A decompressing Iterator for an RLE-encoded [`CompressedBuffer`].
#[derive(Clone)]
pub struct DecompressingIter<'a, B: Copy + PartialEq + Default> {
    current_run: Option<(B, u8)>,
    compressed_buffer_iter: core::slice::Iter<'a, (B, u8)>,
    decompressed_index: usize,
}

impl<'a, B> DecompressingIter<'a, B>
where
    B: Copy + PartialEq + Default,
{
    /// Creates a new decompressing iterator from a vector of runs.
    pub fn new(buffer: &'a CompressedBuffer<B>) -> Self {
        let mut compressed_buffer_iter = buffer.inner.iter();
        let current_run = compressed_buffer_iter.next().map(|&r| r);
        Self {
            current_run,
            compressed_buffer_iter,
            decompressed_index: 0,
        }
    }
}

impl<'a, B: Copy + PartialEq + Default> Iterator for DecompressingIter<'a, B> {
    type Item = B;

    fn next(&mut self) -> Option<Self::Item> {
        let (current_value, items_left_in_run) = self.current_run?;
        if items_left_in_run > 1 {
            self.current_run = Some((current_value, items_left_in_run - 1));
        } else {
            // consuming last element of current_run
            self.current_run = self.compressed_buffer_iter.next().map(|&r| r);
        }
        self.decompressed_index += 1;
        Some(current_value)
    }

    fn nth(&mut self, n: usize) -> Option<B> {
        if n == 0 {
            return self.next();
        }

        let (current_value, items_left_in_run) = self.current_run?;
        if n < (items_left_in_run as usize) {
            // nth item is in current run
            let n_u8 = <usize as TryInto<u8>>::try_into(n).unwrap();
            self.current_run = Some((current_value, items_left_in_run - n_u8));
            self.decompressed_index += n;

            self.next()
        } else {
            // not enough items in current run, skip to next run
            let remaining_n = n - items_left_in_run as usize;
            self.decompressed_index += items_left_in_run as usize;

            let &(next_value, next_run_len) = self.compressed_buffer_iter.next()?;
            assert_ne!(next_run_len, 0, "run with length 0 found");
            self.current_run = Some((next_value, next_run_len));

            self.nth(remaining_n)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buffer_clear() {
        let size = Size::new(128, 4); // 512 pixels total
        let mut buffer = CompressedBuffer::<u8>::new(size, 45);
        buffer.check_integrity().unwrap();

        buffer.clear_and_refill(255);
        assert_eq!(buffer.inner, vec![(255, 255), (255, 255), (255, 2)]);
    }

    #[test]
    fn merge_before() -> Result<(), ()> {
        let size = Size::new(4, 4); // 16 pixels total
        let mut buffer = CompressedBuffer::<u8>::new(size, 30);
        buffer.check_integrity().unwrap();

        buffer.set_at_index(2, 52)?;
        assert_eq!(buffer.inner, vec![(30, 2), (52, 1), (30, 13)]);

        buffer.set_at_index(3, 52)?;
        assert_eq!(buffer.inner, vec![(30, 2), (52, 2), (30, 12)]);
        Ok(())
    }

    #[test]
    fn merge_after() {
        let size = Size::new(4, 4); // 16 pixels total
        let mut buffer = CompressedBuffer::<u8>::new(size, 30);
        buffer.check_integrity().unwrap();

        buffer.set_at_index(2, 52).unwrap();
        assert_eq!(buffer.inner, vec![(30, 2), (52, 1), (30, 13)]);

        buffer.set_at_index(1, 52).unwrap();
        assert_eq!(buffer.inner, vec![(30, 1), (52, 2), (30, 13)]);
    }

    #[test]
    fn merge_before_and_after() -> Result<(), ()> {
        let size = Size::new(128, 2); // 256 pixels total
        let mut buffer = CompressedBuffer::<u8>::new(size, 0);
        buffer.check_integrity()?;
        assert_eq!(buffer.inner, vec![(0, 255), (0, 1)]);

        buffer.set_at_index(0, 27)?;
        assert_eq!(buffer.inner, vec![(27, 1), (0, 255)]);

        buffer.set_at_index(2, 27)?;
        assert_eq!(buffer.inner, vec![(27, 1), (0, 1), (27, 1), (0, 253)]);

        buffer.set_at_index(1, 27)?;
        assert_eq!(buffer.inner, vec![(27, 3), (0, 253)]);
        Ok(())
    }

    #[test]
    fn no_merge_over_255() -> Result<(), ()> {
        let size = Size::new(257, 1);
        let mut buffer = CompressedBuffer::<u8>::new(size, 0);
        buffer.check_integrity()?;
        assert_eq!(buffer.inner, vec![(0, 255), (0, 2)]);
        buffer.set_at_index(254, 3)?;

        assert_eq!(buffer.inner, vec![(0, 254), (3, 1), (0, 2)]);
        buffer.set_at_index(254, 0)?;
        assert_eq!(buffer.inner, vec![(0, 255), (0, 2)]);
        Ok(())
    }

    #[test]
    fn iter() -> Result<(), ()> {
        let width = 64;
        let height = 32;
        let size = Size::new(width, height);
        let mut buffer = CompressedBuffer::<u8>::new(size, 0);
        buffer.check_integrity()?;

        let index1: usize = (height / 2 * width + width / 2) as usize;
        let index2: usize = (width * height - 1) as usize;
        buffer.set_at_index(0, 1)?;
        buffer.set_at_index(index1, 1)?;
        buffer.set_at_index(index2, 1)?;

        buffer.check_integrity()?;
        let iter = DecompressingIter::new(&buffer);

        // check cloned iter
        assert_eq!(iter.clone().nth(0), Some(1));
        assert_eq!(iter.clone().nth(1), Some(0));

        assert_eq!(iter.clone().nth(index1 - 1), Some(0));
        assert_eq!(iter.clone().nth(index1), Some(1));
        assert_eq!(iter.clone().nth(index1 + 1), Some(0));

        assert_eq!(iter.clone().nth(index2), Some(1));

        Ok(())
    }

    #[test]
    fn test_set_contiguous() -> Result<(), ()> {
        let size = Size::new(128, 4); // 512 pixels total
        let mut buffer = CompressedBuffer::<u8>::new(size, 0);
        buffer.check_integrity()?;
        assert_eq!(buffer.inner, vec![(0, 255), (0, 255), (0, 2)]);

        buffer.set_at_index_contiguous(0, 27, 100)?;

        assert_eq!(buffer.inner, vec![(27, 100), (0, 155), (0, 255), (0, 2)]);

        buffer.set_at_index_contiguous(50, 84, 462)?;

        assert_eq!(buffer.inner, vec![(27, 50), (84, 207), (84, 255)]);
        buffer.check_integrity()?;

        let bigger_size = Size::new(128, 8); // 1024 pixels total
        let mut buffer = CompressedBuffer::<u8>::new(bigger_size, 0);
        buffer.check_integrity()?;

        assert_eq!(
            buffer.inner,
            vec![(0, 255), (0, 255), (0, 255), (0, 255), (0, 4)]
        );

        // set the last 550 pixels: 1024 - 550 = 474
        buffer.set_at_index_contiguous(474, 123, 550)?;

        assert_eq!(
            buffer.inner,
            vec![(0, 255), (0, 219), (123, 40), (123, 255), (123, 255)]
        );
        buffer.check_integrity()?;

        Ok(())
    }
}
