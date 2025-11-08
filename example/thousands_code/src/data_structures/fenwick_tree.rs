use std::ops::{Add, AddAssign, Sub, SubAssign};






pub struct FenwickTree<T>
where
    T: Add<Output = T> + AddAssign + Sub<Output = T> + SubAssign + Copy + Default,
{
    
    
    data: Vec<T>,
}


#[derive(Debug, PartialEq, Eq)]
pub enum FenwickTreeError {
    
    IndexOutOfBounds,
    
    InvalidRange,
}

impl<T> FenwickTree<T>
where
    T: Add<Output = T> + AddAssign + Sub<Output = T> + SubAssign + Copy + Default,
{
    
    
    
    
    
    
    
    
    
    
    
    
    pub fn with_capacity(capacity: usize) -> Self {
        FenwickTree {
            data: vec![T::default(); capacity + 1],
        }
    }

    
    
    
    
    
    
    
    
    
    
    
    
    
    pub fn update(&mut self, index: usize, value: T) -> Result<(), FenwickTreeError> {
        if index >= self.data.len() - 1 {
            return Err(FenwickTreeError::IndexOutOfBounds);
        }

        let mut idx = index + 1;
        while idx < self.data.len() {
            self.data[idx] += value;
            idx += lowbit(idx);
        }

        Ok(())
    }

    
    
    
    
    
    
    
    
    
    
    
    
    pub fn prefix_query(&self, index: usize) -> Result<T, FenwickTreeError> {
        if index >= self.data.len() - 1 {
            return Err(FenwickTreeError::IndexOutOfBounds);
        }

        let mut idx = index + 1;
        let mut result = T::default();
        while idx > 0 {
            result += self.data[idx];
            idx -= lowbit(idx);
        }

        Ok(result)
    }

    
    
    
    
    
    
    
    
    
    
    
    
    
    pub fn range_query(&self, left: usize, right: usize) -> Result<T, FenwickTreeError> {
        if left > right || right >= self.data.len() - 1 {
            return Err(FenwickTreeError::InvalidRange);
        }

        let right_query = self.prefix_query(right)?;
        let left_query = if left == 0 {
            T::default()
        } else {
            self.prefix_query(left - 1)?
        };

        Ok(right_query - left_query)
    }

    
    
    
    
    
    
    
    
    
    
    
    
    
    pub fn point_query(&self, index: usize) -> Result<T, FenwickTreeError> {
        if index >= self.data.len() - 1 {
            return Err(FenwickTreeError::IndexOutOfBounds);
        }

        let index_query = self.prefix_query(index)?;
        let prev_query = if index == 0 {
            T::default()
        } else {
            self.prefix_query(index - 1)?
        };

        Ok(index_query - prev_query)
    }

    
    
    
    
    
    
    
    
    
    
    
    
    
    
    pub fn set(&mut self, index: usize, value: T) -> Result<(), FenwickTreeError> {
        self.update(index, value - self.point_query(index)?)
    }
}


















const fn lowbit(x: usize) -> usize {
    x & (!x + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fenwick_tree() {
        let mut fenwick_tree = FenwickTree::with_capacity(10);

        assert_eq!(fenwick_tree.update(0, 5), Ok(()));
        assert_eq!(fenwick_tree.update(1, 3), Ok(()));
        assert_eq!(fenwick_tree.update(2, -2), Ok(()));
        assert_eq!(fenwick_tree.update(3, 6), Ok(()));
        assert_eq!(fenwick_tree.update(4, -4), Ok(()));
        assert_eq!(fenwick_tree.update(5, 7), Ok(()));
        assert_eq!(fenwick_tree.update(6, -1), Ok(()));
        assert_eq!(fenwick_tree.update(7, 2), Ok(()));
        assert_eq!(fenwick_tree.update(8, -3), Ok(()));
        assert_eq!(fenwick_tree.update(9, 4), Ok(()));
        assert_eq!(fenwick_tree.set(3, 10), Ok(()));
        assert_eq!(fenwick_tree.point_query(3), Ok(10));
        assert_eq!(fenwick_tree.set(5, 0), Ok(()));
        assert_eq!(fenwick_tree.point_query(5), Ok(0));
        assert_eq!(
            fenwick_tree.update(10, 11),
            Err(FenwickTreeError::IndexOutOfBounds)
        );
        assert_eq!(
            fenwick_tree.set(10, 11),
            Err(FenwickTreeError::IndexOutOfBounds)
        );

        assert_eq!(fenwick_tree.prefix_query(0), Ok(5));
        assert_eq!(fenwick_tree.prefix_query(1), Ok(8));
        assert_eq!(fenwick_tree.prefix_query(2), Ok(6));
        assert_eq!(fenwick_tree.prefix_query(3), Ok(16));
        assert_eq!(fenwick_tree.prefix_query(4), Ok(12));
        assert_eq!(fenwick_tree.prefix_query(5), Ok(12));
        assert_eq!(fenwick_tree.prefix_query(6), Ok(11));
        assert_eq!(fenwick_tree.prefix_query(7), Ok(13));
        assert_eq!(fenwick_tree.prefix_query(8), Ok(10));
        assert_eq!(fenwick_tree.prefix_query(9), Ok(14));
        assert_eq!(
            fenwick_tree.prefix_query(10),
            Err(FenwickTreeError::IndexOutOfBounds)
        );

        assert_eq!(fenwick_tree.range_query(0, 4), Ok(12));
        assert_eq!(fenwick_tree.range_query(3, 7), Ok(7));
        assert_eq!(fenwick_tree.range_query(2, 5), Ok(4));
        assert_eq!(
            fenwick_tree.range_query(4, 3),
            Err(FenwickTreeError::InvalidRange)
        );
        assert_eq!(
            fenwick_tree.range_query(2, 10),
            Err(FenwickTreeError::InvalidRange)
        );

        assert_eq!(fenwick_tree.point_query(0), Ok(5));
        assert_eq!(fenwick_tree.point_query(4), Ok(-4));
        assert_eq!(fenwick_tree.point_query(9), Ok(4));
        assert_eq!(
            fenwick_tree.point_query(10),
            Err(FenwickTreeError::IndexOutOfBounds)
        );
    }
}
