use std::cmp::min;


#[derive(Debug, PartialEq, Eq)]
pub enum MatrixError {
    
    EmptyMatrix,
    
    NonRectangularMatrix,
}





















pub fn minimum_cost_path(matrix: Vec<Vec<usize>>) -> Result<usize, MatrixError> {
    
    if !matrix.iter().all(|row| row.len() == matrix[0].len()) {
        return Err(MatrixError::NonRectangularMatrix);
    }

    
    if matrix.is_empty() || matrix.iter().all(|row| row.is_empty()) {
        return Err(MatrixError::EmptyMatrix);
    }

    
    let mut cost = matrix[0]
        .iter()
        .scan(0, |acc, &val| {
            *acc += val;
            Some(*acc)
        })
        .collect::<Vec<_>>();

    
    for row in matrix.iter().skip(1) {
        
        cost[0] += row[0];

        
        for col in 1..matrix[0].len() {
            cost[col] = row[col] + min(cost[col - 1], cost[col]);
        }
    }

    
    Ok(cost[matrix[0].len() - 1])
}

#[cfg(test)]
mod tests {
    use super::*;

    macro_rules! minimum_cost_path_tests {
        ($($name:ident: $test_case:expr,)*) => {
            $(
                #[test]
                fn $name() {
                    let (matrix, expected) = $test_case;
                    assert_eq!(minimum_cost_path(matrix), expected);
                }
            )*
        };
    }

    minimum_cost_path_tests! {
        basic: (
            vec![
                vec![2, 1, 4],
                vec![2, 1, 3],
                vec![3, 2, 1]
            ],
            Ok(7)
        ),
        single_element: (
            vec![
                vec![5]
            ],
            Ok(5)
        ),
        single_row: (
            vec![
                vec![1, 3, 2, 1, 5]
            ],
            Ok(12)
        ),
        single_column: (
            vec![
                vec![1],
                vec![3],
                vec![2],
                vec![1],
                vec![5]
            ],
            Ok(12)
        ),
        large_matrix: (
            vec![
                vec![1, 3, 1, 5],
                vec![2, 1, 4, 2],
                vec![3, 2, 1, 3],
                vec![4, 3, 2, 1]
            ],
            Ok(10)
        ),
        uniform_matrix: (
            vec![
                vec![1, 1, 1],
                vec![1, 1, 1],
                vec![1, 1, 1]
            ],
            Ok(5)
        ),
        increasing_values: (
            vec![
                vec![1, 2, 3],
                vec![4, 5, 6],
                vec![7, 8, 9]
            ],
            Ok(21)
        ),
        high_cost_path: (
            vec![
                vec![1, 100, 1],
                vec![1, 100, 1],
                vec![1, 1, 1]
            ],
            Ok(5)
        ),
        complex_matrix: (
            vec![
                vec![5, 9, 6, 8],
                vec![1, 4, 7, 3],
                vec![2, 1, 8, 2],
                vec![3, 6, 9, 4]
            ],
            Ok(23)
        ),
        empty_matrix: (
            vec![],
            Err(MatrixError::EmptyMatrix)
        ),
        empty_row: (
            vec![
                vec![],
                vec![],
                vec![]
            ],
            Err(MatrixError::EmptyMatrix)
        ),
        non_rectangular: (
            vec![
                vec![1, 2, 3],
                vec![4, 5],
                vec![6, 7, 8]
            ],
            Err(MatrixError::NonRectangularMatrix)
        ),
    }
}
