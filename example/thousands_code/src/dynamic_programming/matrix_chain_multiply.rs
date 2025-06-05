











#[derive(Debug, PartialEq)]
pub enum MatrixChainMultiplicationError {
    EmptyDimensions,
    InsufficientDimensions,
}

















pub fn matrix_chain_multiply(
    dimensions: Vec<usize>,
) -> Result<usize, MatrixChainMultiplicationError> {
    if dimensions.is_empty() {
        return Err(MatrixChainMultiplicationError::EmptyDimensions);
    }

    if dimensions.len() == 1 {
        return Err(MatrixChainMultiplicationError::InsufficientDimensions);
    }

    let mut min_operations = vec![vec![0; dimensions.len()]; dimensions.len()];

    (2..dimensions.len()).for_each(|chain_len| {
        (0..dimensions.len() - chain_len).for_each(|start| {
            let end = start + chain_len;
            min_operations[start][end] = (start + 1..end)
                .map(|split| {
                    min_operations[start][split]
                        + min_operations[split][end]
                        + dimensions[start] * dimensions[split] * dimensions[end]
                })
                .min()
                .unwrap_or(usize::MAX);
        });
    });

    Ok(min_operations[0][dimensions.len() - 1])
}

#[cfg(test)]
mod tests {
    use super::*;

    macro_rules! test_cases {
        ($($name:ident: $test_case:expr,)*) => {
            $(
                #[test]
                fn $name() {
                    let (input, expected) = $test_case;
                    assert_eq!(matrix_chain_multiply(input.clone()), expected);
                    assert_eq!(matrix_chain_multiply(input.into_iter().rev().collect()), expected);
                }
            )*
        };
    }

    test_cases! {
        basic_chain_of_matrices: (vec![1, 2, 3, 4], Ok(18)),
        chain_of_large_matrices: (vec![40, 20, 30, 10, 30], Ok(26000)),
        long_chain_of_matrices: (vec![1, 2, 3, 4, 3, 5, 7, 6, 10], Ok(182)),
        complex_chain_of_matrices: (vec![4, 10, 3, 12, 20, 7], Ok(1344)),
        empty_dimensions_input: (vec![], Err(MatrixChainMultiplicationError::EmptyDimensions)),
        single_dimensions_input: (vec![10], Err(MatrixChainMultiplicationError::InsufficientDimensions)),
        single_matrix_input: (vec![10, 20], Ok(0)),
    }
}
