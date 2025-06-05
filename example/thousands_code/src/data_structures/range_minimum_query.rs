










use std::cmp::PartialOrd;


#[derive(Debug, PartialEq, Eq)]
pub enum RangeError {
    
    InvalidRange,
    
    IndexOutOfBound,
}


pub struct RangeMinimumQuery<T: PartialOrd + Copy> {
    
    data: Vec<T>,
    
    
    sparse_table: Vec<Vec<usize>>,
}

impl<T: PartialOrd + Copy> RangeMinimumQuery<T> {
    
    
    
    
    
    
    
    
    
    pub fn new(input: &[T]) -> RangeMinimumQuery<T> {
        RangeMinimumQuery {
            data: input.to_vec(),
            sparse_table: build_sparse_table(input),
        }
    }

    
    
    
    
    
    
    
    
    
    
    
    
    pub fn get_range_min(&self, start: usize, end: usize) -> Result<T, RangeError> {
        
        if start >= end {
            return Err(RangeError::InvalidRange);
        }
        if start >= self.data.len() || end > self.data.len() {
            return Err(RangeError::IndexOutOfBound);
        }

        
        let log_len = (end - start).ilog2() as usize;
        let idx: usize = end - (1 << log_len);

        
        let min_idx_start = self.sparse_table[log_len][start];
        let min_idx_end = self.sparse_table[log_len][idx];

        
        if self.data[min_idx_start] < self.data[min_idx_end] {
            Ok(self.data[min_idx_start])
        } else {
            Ok(self.data[min_idx_end])
        }
    }
}











fn build_sparse_table<T: PartialOrd>(data: &[T]) -> Vec<Vec<usize>> {
    let mut sparse_table: Vec<Vec<usize>> = vec![(0..data.len()).collect()];
    let len = data.len();

    
    for log_len in 1..=len.ilog2() {
        let mut row = Vec::new();
        for idx in 0..=len - (1 << log_len) {
            let min_idx_start = sparse_table[sparse_table.len() - 1][idx];
            let min_idx_end = sparse_table[sparse_table.len() - 1][idx + (1 << (log_len - 1))];
            if data[min_idx_start] < data[min_idx_end] {
                row.push(min_idx_start);
            } else {
                row.push(min_idx_end);
            }
        }
        sparse_table.push(row);
    }

    sparse_table
}

#[cfg(test)]
mod tests {
    use super::*;

    macro_rules! test_build_sparse_table {
        ($($name:ident: $inputs:expr,)*) => {
            $(
                #[test]
                fn $name() {
                    let (data, expected) = $inputs;
                    assert_eq!(build_sparse_table(&data), expected);
                }
            )*
        }
    }

    test_build_sparse_table! {
        small: (
            [1, 6, 3],
            vec![
                vec![0, 1, 2],
                vec![0, 2]
            ]
        ),
        medium: (
            [1, 3, 6, 123, 7, 235, 3, -4, 6, 2],
            vec![
                vec![0, 1, 2, 3, 4, 5, 6, 7, 8, 9],
                vec![0, 1, 2, 4, 4, 6, 7, 7, 9],
                vec![0, 1, 2, 6, 7, 7, 7],
                vec![7, 7, 7]
            ]
        ),
        large: (
            [20, 13, -13, 2, 3634, -2, 56, 3, 67, 8, 23, 0, -23, 1, 5, 85, 3, 24, 5, -10, 3, 4, 20],
            vec![
                vec![0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22],
                vec![1, 2, 2, 3, 5, 5, 7, 7, 9, 9, 11, 12, 12, 13, 14, 16, 16, 18, 19, 19, 20, 21],
                vec![2, 2, 2, 5, 5, 5, 7, 7, 11, 12, 12, 12, 12, 13, 16, 16, 19, 19, 19, 19],
                vec![2, 2, 2, 5, 5, 12, 12, 12, 12, 12, 12, 12, 12, 19, 19, 19],
                vec![12, 12, 12, 12, 12, 12, 12, 12]
            ]
        ),
    }

    #[test]
    fn simple_query_tests() {
        let rmq = RangeMinimumQuery::new(&[1, 3, 6, 123, 7, 235, 3, -4, 6, 2]);

        assert_eq!(rmq.get_range_min(1, 6), Ok(3));
        assert_eq!(rmq.get_range_min(0, 10), Ok(-4));
        assert_eq!(rmq.get_range_min(8, 9), Ok(6));
        assert_eq!(rmq.get_range_min(4, 3), Err(RangeError::InvalidRange));
        assert_eq!(rmq.get_range_min(0, 1000), Err(RangeError::IndexOutOfBound));
        assert_eq!(
            rmq.get_range_min(1000, 1001),
            Err(RangeError::IndexOutOfBound)
        );
    }

    #[test]
    fn float_query_tests() {
        let rmq = RangeMinimumQuery::new(&[0.4, -2.3, 0.0, 234.22, 12.2, -3.0]);

        assert_eq!(rmq.get_range_min(0, 6), Ok(-3.0));
        assert_eq!(rmq.get_range_min(0, 4), Ok(-2.3));
        assert_eq!(rmq.get_range_min(3, 5), Ok(12.2));
        assert_eq!(rmq.get_range_min(2, 3), Ok(0.0));
        assert_eq!(rmq.get_range_min(4, 3), Err(RangeError::InvalidRange));
        assert_eq!(rmq.get_range_min(0, 1000), Err(RangeError::IndexOutOfBound));
        assert_eq!(
            rmq.get_range_min(1000, 1001),
            Err(RangeError::IndexOutOfBound)
        );
    }
}
