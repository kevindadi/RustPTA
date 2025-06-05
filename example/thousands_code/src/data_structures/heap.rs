







use std::{cmp::Ord, slice::Iter};







pub struct Heap<T> {
    items: Vec<T>,
    comparator: fn(&T, &T) -> bool,
}

impl<T> Heap<T> {
    
    
    
    
    
    
    
    pub fn new(comparator: fn(&T, &T) -> bool) -> Self {
        Self {
            items: vec![],
            comparator,
        }
    }

    
    
    
    
    
    
    
    
    pub fn from_vec(items: Vec<T>, comparator: fn(&T, &T) -> bool) -> Self {
        let mut heap = Self { items, comparator };
        heap.build_heap();
        heap
    }

    
    fn build_heap(&mut self) {
        let last_parent_idx = (self.len() / 2).wrapping_sub(1);
        for idx in (0..=last_parent_idx).rev() {
            self.heapify_down(idx);
        }
    }

    
    
    
    
    pub fn len(&self) -> usize {
        self.items.len()
    }

    
    
    
    
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    
    
    
    
    pub fn add(&mut self, value: T) {
        self.items.push(value);
        self.heapify_up(self.len() - 1);
    }

    
    
    
    
    pub fn pop(&mut self) -> Option<T> {
        if self.is_empty() {
            return None;
        }
        let next = Some(self.items.swap_remove(0));
        if !self.is_empty() {
            self.heapify_down(0);
        }
        next
    }

    
    
    
    
    pub fn iter(&self) -> Iter<'_, T> {
        self.items.iter()
    }

    
    
    
    
    fn heapify_up(&mut self, mut idx: usize) {
        while let Some(pdx) = self.parent_idx(idx) {
            if (self.comparator)(&self.items[idx], &self.items[pdx]) {
                self.items.swap(idx, pdx);
                idx = pdx;
            } else {
                break;
            }
        }
    }

    
    
    
    
    fn heapify_down(&mut self, mut idx: usize) {
        while self.children_present(idx) {
            let cdx = {
                if self.right_child_idx(idx) >= self.len() {
                    self.left_child_idx(idx)
                } else {
                    let ldx = self.left_child_idx(idx);
                    let rdx = self.right_child_idx(idx);
                    if (self.comparator)(&self.items[ldx], &self.items[rdx]) {
                        ldx
                    } else {
                        rdx
                    }
                }
            };

            if (self.comparator)(&self.items[cdx], &self.items[idx]) {
                self.items.swap(idx, cdx);
                idx = cdx;
            } else {
                break;
            }
        }
    }

    
    
    
    
    
    
    
    fn parent_idx(&self, idx: usize) -> Option<usize> {
        if idx > 0 {
            Some((idx - 1) / 2)
        } else {
            None
        }
    }

    
    
    
    
    
    
    
    fn children_present(&self, idx: usize) -> bool {
        self.left_child_idx(idx) < self.len()
    }

    
    
    
    
    
    
    
    fn left_child_idx(&self, idx: usize) -> usize {
        idx * 2 + 1
    }

    
    
    
    
    
    
    
    fn right_child_idx(&self, idx: usize) -> usize {
        self.left_child_idx(idx) + 1
    }
}

impl<T> Heap<T>
where
    T: Ord,
{
    
    
    
    
    pub fn new_min() -> Heap<T> {
        Self::new(|a, b| a < b)
    }

    
    
    
    
    pub fn new_max() -> Heap<T> {
        Self::new(|a, b| a > b)
    }

    
    
    
    
    
    
    
    pub fn from_vec_min(items: Vec<T>) -> Heap<T> {
        Self::from_vec(items, |a, b| a < b)
    }

    
    
    
    
    
    
    
    pub fn from_vec_max(items: Vec<T>) -> Heap<T> {
        Self::from_vec(items, |a, b| a > b)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_heap() {
        let mut heap: Heap<i32> = Heap::new_max();
        assert_eq!(heap.pop(), None);
    }

    #[test]
    fn test_min_heap() {
        let mut heap = Heap::new_min();
        heap.add(4);
        heap.add(2);
        heap.add(9);
        heap.add(11);
        assert_eq!(heap.len(), 4);
        assert_eq!(heap.pop(), Some(2));
        assert_eq!(heap.pop(), Some(4));
        assert_eq!(heap.pop(), Some(9));
        heap.add(1);
        assert_eq!(heap.pop(), Some(1));
        assert_eq!(heap.pop(), Some(11));
        assert_eq!(heap.pop(), None);
    }

    #[test]
    fn test_max_heap() {
        let mut heap = Heap::new_max();
        heap.add(4);
        heap.add(2);
        heap.add(9);
        heap.add(11);
        assert_eq!(heap.len(), 4);
        assert_eq!(heap.pop(), Some(11));
        assert_eq!(heap.pop(), Some(9));
        assert_eq!(heap.pop(), Some(4));
        heap.add(1);
        assert_eq!(heap.pop(), Some(2));
        assert_eq!(heap.pop(), Some(1));
        assert_eq!(heap.pop(), None);
    }

    #[test]
    fn test_iter_heap() {
        let mut heap = Heap::new_min();
        heap.add(4);
        heap.add(2);
        heap.add(9);
        heap.add(11);

        let mut iter = heap.iter();
        assert_eq!(iter.next(), Some(&2));
        assert_eq!(iter.next(), Some(&4));
        assert_eq!(iter.next(), Some(&9));
        assert_eq!(iter.next(), Some(&11));
        assert_eq!(iter.next(), None);

        assert_eq!(heap.len(), 4);
        assert_eq!(heap.pop(), Some(2));
        assert_eq!(heap.pop(), Some(4));
        assert_eq!(heap.pop(), Some(9));
        assert_eq!(heap.pop(), Some(11));
        assert_eq!(heap.pop(), None);
    }

    #[test]
    fn test_from_vec_min() {
        let vec = vec![3, 1, 4, 1, 5, 9, 2, 6, 5];
        let mut heap = Heap::from_vec_min(vec);
        assert_eq!(heap.len(), 9);
        assert_eq!(heap.pop(), Some(1));
        assert_eq!(heap.pop(), Some(1));
        assert_eq!(heap.pop(), Some(2));
        heap.add(0);
        assert_eq!(heap.pop(), Some(0));
    }

    #[test]
    fn test_from_vec_max() {
        let vec = vec![3, 1, 4, 1, 5, 9, 2, 6, 5];
        let mut heap = Heap::from_vec_max(vec);
        assert_eq!(heap.len(), 9);
        assert_eq!(heap.pop(), Some(9));
        assert_eq!(heap.pop(), Some(6));
        assert_eq!(heap.pop(), Some(5));
        heap.add(10);
        assert_eq!(heap.pop(), Some(10));
    }
}
