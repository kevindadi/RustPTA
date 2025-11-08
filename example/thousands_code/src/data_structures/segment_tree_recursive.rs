use std::fmt::Debug;
use std::ops::Range;


#[derive(Debug, PartialEq, Eq)]
pub enum SegmentTreeError {
    
    IndexOutOfBounds,
    
    InvalidRange,
}



pub struct SegmentTree<T, F>
where
    T: Debug + Default + Ord + Copy,
    F: Fn(T, T) -> T,
{
    
    size: usize,
    
    nodes: Vec<T>,
    
    merge_fn: F,
}

impl<T, F> SegmentTree<T, F>
where
    T: Debug + Default + Ord + Copy,
    F: Fn(T, T) -> T,
{
    
    
    
    
    
    
    
    
    
    
    pub fn from_vec(arr: &[T], merge_fn: F) -> Self {
        let size = arr.len();
        let mut seg_tree = SegmentTree {
            size,
            nodes: vec![T::default(); 4 * size],
            merge_fn,
        };
        if size != 0 {
            seg_tree.build_recursive(arr, 1, 0..size);
        }
        seg_tree
    }

    
    
    
    
    
    
    
    fn build_recursive(&mut self, arr: &[T], node_idx: usize, node_range: Range<usize>) {
        if node_range.end - node_range.start == 1 {
            self.nodes[node_idx] = arr[node_range.start];
        } else {
            let mid = node_range.start + (node_range.end - node_range.start) / 2;
            self.build_recursive(arr, 2 * node_idx, node_range.start..mid);
            self.build_recursive(arr, 2 * node_idx + 1, mid..node_range.end);
            self.nodes[node_idx] =
                (self.merge_fn)(self.nodes[2 * node_idx], self.nodes[2 * node_idx + 1]);
        }
    }

    
    
    
    
    
    
    
    
    
    
    
    
    pub fn query(&self, target_range: Range<usize>) -> Result<Option<T>, SegmentTreeError> {
        if target_range.start >= self.size || target_range.end > self.size {
            return Err(SegmentTreeError::InvalidRange);
        }
        Ok(self.query_recursive(1, 0..self.size, &target_range))
    }

    
    
    
    
    
    
    
    
    
    
    
    
    fn query_recursive(
        &self,
        node_idx: usize,
        tree_range: Range<usize>,
        target_range: &Range<usize>,
    ) -> Option<T> {
        if tree_range.start >= target_range.end || tree_range.end <= target_range.start {
            return None;
        }
        if tree_range.start >= target_range.start && tree_range.end <= target_range.end {
            return Some(self.nodes[node_idx]);
        }
        let mid = tree_range.start + (tree_range.end - tree_range.start) / 2;
        let left_res = self.query_recursive(node_idx * 2, tree_range.start..mid, target_range);
        let right_res = self.query_recursive(node_idx * 2 + 1, mid..tree_range.end, target_range);
        match (left_res, right_res) {
            (None, None) => None,
            (None, Some(r)) => Some(r),
            (Some(l), None) => Some(l),
            (Some(l), Some(r)) => Some((self.merge_fn)(l, r)),
        }
    }

    
    
    
    
    
    
    
    
    
    
    
    pub fn update(&mut self, target_idx: usize, val: T) -> Result<(), SegmentTreeError> {
        if target_idx >= self.size {
            return Err(SegmentTreeError::IndexOutOfBounds);
        }
        self.update_recursive(1, 0..self.size, target_idx, val);
        Ok(())
    }

    
    
    
    
    
    
    
    
    fn update_recursive(
        &mut self,
        node_idx: usize,
        tree_range: Range<usize>,
        target_idx: usize,
        val: T,
    ) {
        if tree_range.start > target_idx || tree_range.end <= target_idx {
            return;
        }
        if tree_range.end - tree_range.start <= 1 && tree_range.start == target_idx {
            self.nodes[node_idx] = val;
            return;
        }
        let mid = tree_range.start + (tree_range.end - tree_range.start) / 2;
        self.update_recursive(node_idx * 2, tree_range.start..mid, target_idx, val);
        self.update_recursive(node_idx * 2 + 1, mid..tree_range.end, target_idx, val);
        self.nodes[node_idx] =
            (self.merge_fn)(self.nodes[node_idx * 2], self.nodes[node_idx * 2 + 1]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cmp::{max, min};

    #[test]
    fn test_min_segments() {
        let vec = vec![-30, 2, -4, 7, 3, -5, 6, 11, -20, 9, 14, 15, 5, 2, -8];
        let mut min_seg_tree = SegmentTree::from_vec(&vec, min);
        assert_eq!(min_seg_tree.query(4..7), Ok(Some(-5)));
        assert_eq!(min_seg_tree.query(0..vec.len()), Ok(Some(-30)));
        assert_eq!(min_seg_tree.query(0..2), Ok(Some(-30)));
        assert_eq!(min_seg_tree.query(1..3), Ok(Some(-4)));
        assert_eq!(min_seg_tree.query(1..7), Ok(Some(-5)));
        assert_eq!(min_seg_tree.update(5, 10), Ok(()));
        assert_eq!(min_seg_tree.update(14, -8), Ok(()));
        assert_eq!(min_seg_tree.query(4..7), Ok(Some(3)));
        assert_eq!(
            min_seg_tree.update(15, 100),
            Err(SegmentTreeError::IndexOutOfBounds)
        );
        assert_eq!(min_seg_tree.query(5..5), Ok(None));
        assert_eq!(
            min_seg_tree.query(10..16),
            Err(SegmentTreeError::InvalidRange)
        );
        assert_eq!(
            min_seg_tree.query(15..20),
            Err(SegmentTreeError::InvalidRange)
        );
    }

    #[test]
    fn test_max_segments() {
        let vec = vec![1, 2, -4, 7, 3, -5, 6, 11, -20, 9, 14, 15, 5, 2, -8];
        let mut max_seg_tree = SegmentTree::from_vec(&vec, max);
        assert_eq!(max_seg_tree.query(0..vec.len()), Ok(Some(15)));
        assert_eq!(max_seg_tree.query(3..5), Ok(Some(7)));
        assert_eq!(max_seg_tree.query(4..8), Ok(Some(11)));
        assert_eq!(max_seg_tree.query(8..10), Ok(Some(9)));
        assert_eq!(max_seg_tree.query(9..12), Ok(Some(15)));
        assert_eq!(max_seg_tree.update(4, 10), Ok(()));
        assert_eq!(max_seg_tree.update(14, -8), Ok(()));
        assert_eq!(max_seg_tree.query(3..5), Ok(Some(10)));
        assert_eq!(
            max_seg_tree.update(15, 100),
            Err(SegmentTreeError::IndexOutOfBounds)
        );
        assert_eq!(max_seg_tree.query(5..5), Ok(None));
        assert_eq!(
            max_seg_tree.query(10..16),
            Err(SegmentTreeError::InvalidRange)
        );
        assert_eq!(
            max_seg_tree.query(15..20),
            Err(SegmentTreeError::InvalidRange)
        );
    }

    #[test]
    fn test_sum_segments() {
        let vec = vec![1, 2, -4, 7, 3, -5, 6, 11, -20, 9, 14, 15, 5, 2, -8];
        let mut sum_seg_tree = SegmentTree::from_vec(&vec, |a, b| a + b);
        assert_eq!(sum_seg_tree.query(0..vec.len()), Ok(Some(38)));
        assert_eq!(sum_seg_tree.query(1..4), Ok(Some(5)));
        assert_eq!(sum_seg_tree.query(4..7), Ok(Some(4)));
        assert_eq!(sum_seg_tree.query(6..9), Ok(Some(-3)));
        assert_eq!(sum_seg_tree.query(9..vec.len()), Ok(Some(37)));
        assert_eq!(sum_seg_tree.update(5, 10), Ok(()));
        assert_eq!(sum_seg_tree.update(14, -8), Ok(()));
        assert_eq!(sum_seg_tree.query(4..7), Ok(Some(19)));
        assert_eq!(
            sum_seg_tree.update(15, 100),
            Err(SegmentTreeError::IndexOutOfBounds)
        );
        assert_eq!(sum_seg_tree.query(5..5), Ok(None));
        assert_eq!(
            sum_seg_tree.query(10..16),
            Err(SegmentTreeError::InvalidRange)
        );
        assert_eq!(
            sum_seg_tree.query(15..20),
            Err(SegmentTreeError::InvalidRange)
        );
    }
}
