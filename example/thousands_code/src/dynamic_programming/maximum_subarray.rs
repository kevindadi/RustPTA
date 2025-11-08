




#[derive(Debug, PartialEq)]
pub enum MaximumSubarrayError {
    EmptyArray,
}




















pub fn maximum_subarray(array: &[isize]) -> Result<isize, MaximumSubarrayError> {
    if array.is_empty() {
        return Err(MaximumSubarrayError::EmptyArray);
    }

    let mut cur_sum = array[0];
    let mut max_sum = cur_sum;

    for &x in &array[1..] {
        cur_sum = (cur_sum + x).max(x);
        max_sum = max_sum.max(cur_sum);
    }

    Ok(max_sum)
}

#[cfg(test)]
mod tests {
    use super::*;

    macro_rules! maximum_subarray_tests {
        ($($name:ident: $tc:expr,)*) => {
            $(
                #[test]
                fn $name() {
                    let (array, expected) = $tc;
                    assert_eq!(maximum_subarray(&array), expected);
                }
            )*
        }
    }

    maximum_subarray_tests! {
        test_all_non_negative: (vec![1, 0, 5, 8], Ok(14)),
        test_all_negative: (vec![-3, -1, -8, -2], Ok(-1)),
        test_mixed_negative_and_positive: (vec![-4, 3, -2, 5, -8], Ok(6)),
        test_single_element_positive: (vec![6], Ok(6)),
        test_single_element_negative: (vec![-6], Ok(-6)),
        test_mixed_elements: (vec![-2, 1, -3, 4, -1, 2, 1, -5, 4], Ok(6)),
        test_empty_array: (vec![], Err(MaximumSubarrayError::EmptyArray)),
        test_all_zeroes: (vec![0, 0, 0, 0], Ok(0)),
        test_single_zero: (vec![0], Ok(0)),
        test_alternating_signs: (vec![3, -2, 5, -1], Ok(6)),
        test_all_negatives_with_one_positive: (vec![-3, -4, 1, -7, -2], Ok(1)),
        test_all_positives_with_one_negative: (vec![3, 4, -1, 7, 2], Ok(15)),
        test_all_positives: (vec![2, 3, 1, 5], Ok(11)),
        test_large_values: (vec![1000, -500, 1000, -500, 1000], Ok(2000)),
        test_large_array: ((0..1000).collect::<Vec<_>>(), Ok(499500)),
        test_large_negative_array: ((0..1000).map(|x| -x).collect::<Vec<_>>(), Ok(0)),
        test_single_large_positive: (vec![1000000], Ok(1000000)),
        test_single_large_negative: (vec![-1000000], Ok(-1000000)),
    }
}
