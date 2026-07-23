use std::collections::VecDeque;

const CAPACITY: usize = 64;

pub(crate) struct BoundedEvidenceRing<T> {
    items: VecDeque<T>,
}

impl<T> BoundedEvidenceRing<T> {
    pub(crate) fn new() -> Self {
        Self {
            items: VecDeque::with_capacity(CAPACITY),
        }
    }

    pub(crate) fn push(&mut self, item: T) {
        if self.items.len() == CAPACITY {
            let _ = self.pop();
        }
        self.items.push_back(item);
    }

    fn pop(&mut self) -> Option<T> {
        self.items.pop_front()
    }

    pub(crate) fn drain(&mut self) -> impl Iterator<Item = T> + '_ {
        self.items.drain(..)
    }
}

#[cfg(test)]
mod tests {
    use super::{BoundedEvidenceRing, CAPACITY};

    #[test]
    fn pop_is_fifo() {
        let mut ring = BoundedEvidenceRing::new();
        ring.push(3);
        ring.push(1);
        ring.push(2);

        assert_eq!(ring.pop(), Some(3));
        assert_eq!(ring.pop(), Some(1));
        assert_eq!(ring.pop(), Some(2));
        assert_eq!(ring.pop(), None);
    }

    #[test]
    fn retains_exactly_the_fixed_capacity() {
        let mut ring = BoundedEvidenceRing::new();
        for value in 0..CAPACITY {
            ring.push(value);
        }

        assert_eq!(ring.items.len(), CAPACITY);
    }

    #[test]
    fn sixty_fifth_push_drops_the_oldest_item() {
        let mut ring = BoundedEvidenceRing::new();
        for value in 0..=CAPACITY {
            ring.push(value);
        }

        assert_eq!(ring.items.len(), CAPACITY);
        assert_eq!(ring.pop(), Some(1));
        assert_eq!(ring.items.back(), Some(&CAPACITY));
    }

    #[test]
    fn drain_is_fifo_and_empties_the_ring() {
        let mut ring = BoundedEvidenceRing::new();
        ring.push(8);
        ring.push(5);
        ring.push(13);

        assert_eq!(ring.drain().collect::<Vec<_>>(), vec![8, 5, 13]);
        assert_eq!(ring.pop(), None);
    }
}
