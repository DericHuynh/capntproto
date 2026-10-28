//! Lowest vacant operation slot, without inspecting live owners in the first word.
//! Like the engine's FreeSlots, the bitmap precedes every larger slot. Larger
//! arrays keep their existing scan, without another allocation or per-slot field.

pub(super) struct Vacancies {
    first: usize,
    small: usize,
}

impl Default for Vacancies {
    fn default() -> Self {
        Self {
            first: 0,
            // Unissued positions are vacant too. Committing the current lowest
            // position can advance to at most the initialized array's end.
            small: usize::MAX,
        }
    }
}

impl Vacancies {
    pub(super) fn next(&self) -> usize {
        self.first
    }

    /// Commit only after admission has installed its owner. Rejected admission
    /// leaves the inspected vacancy available without rollback.
    pub(super) fn commit<T>(&mut self, index: usize, entries: &[Option<T>]) {
        debug_assert_eq!(index, self.first);
        debug_assert!(entries[index].is_some());
        // A larger selected index implies that no small vacancy remains, so a
        // wrapping shift also safely leaves the empty bitmap unchanged there.
        self.small &= !1usize.wrapping_shl(index as u32);
        self.first = if self.small != 0 {
            self.small.trailing_zeros() as usize
        } else {
            let start = (index + 1).max(usize::BITS as usize);
            entries[start..]
                .iter()
                .position(Option::is_none)
                .map_or(entries.len(), |offset| start + offset)
        };
    }

    pub(super) fn release(&mut self, index: usize) {
        self.small |= usize::from(index < usize::BITS as usize).wrapping_shl(index as u32);
        self.first = self.first.min(index);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mixed_admission_and_retirement_match_lowest_vacancy_without_allocation() {
        for length in [0, 1, 8, 31, 32, 33, 63, 64, 65, 128, 257] {
            let mut entries = [None; 257];
            let mut vacancies = Vacancies::default();
            let mut random = 17u32;
            let allocation = allocation_counter::measure(|| {
                for _ in 0..if cfg!(miri) { 100 } else { 10_000 } {
                    let expected = entries[..length]
                        .iter()
                        .position(Option::is_none)
                        .unwrap_or(length);
                    assert_eq!(vacancies.next(), expected);
                    // Failed/preflight-only admission commits no slot.
                    assert_eq!(vacancies.next(), expected);
                    if length == 0 {
                        continue;
                    }
                    random = random.wrapping_mul(1664525).wrapping_add(1013904223);
                    let index = random as usize % length;
                    if entries[index].take().is_some() {
                        vacancies.release(index);
                    } else if expected < length {
                        entries[expected] = Some(expected);
                        vacancies.commit(expected, &entries[..length]);
                    }
                }
            });
            assert_eq!(allocation.count_total, 0);
            assert_eq!(allocation.bytes_total, 0);
        }
    }

    #[test]
    fn growth_and_retirement_cross_the_bitmap_boundary_in_original_order() {
        const WORD: usize = usize::BITS as usize;
        let mut entries = [None; 2 * WORD + 3];
        let mut vacancies = Vacancies::default();
        for index in 0..entries.len() {
            assert_eq!(vacancies.next(), index);
            entries[index] = Some(index);
            vacancies.commit(index, &entries[..=index]);
        }
        for index in [WORD + 2, WORD, WORD - 1, 0, 2 * WORD + 2] {
            entries[index] = None;
            vacancies.release(index);
        }
        for index in [0, WORD - 1, WORD, WORD + 2, 2 * WORD + 2] {
            assert_eq!(vacancies.next(), index);
            entries[index] = Some(index);
            vacancies.commit(index, &entries);
        }
        assert_eq!(vacancies.next(), entries.len());
    }
}
