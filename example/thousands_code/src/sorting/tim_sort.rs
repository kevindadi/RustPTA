




use crate::sorting::insertion_sort;
use std::cmp;

static MIN_MERGE: usize = 32;












fn compute_min_run_length(array_length: usize) -> usize {
    let mut remaining_length = array_length;
    let mut result = 0;

    while remaining_length >= MIN_MERGE {
        result |= remaining_length & 1;
        remaining_length >>= 1;
    }

    remaining_length + result
}











fn merge<T: Ord + Copy>(arr: &mut [T], left: usize, mid: usize, right: usize) {
    let left_slice = arr[left..=mid].to_vec();
    let right_slice = arr[mid + 1..=right].to_vec();
    let mut i = 0;
    let mut j = 0;
    let mut k = left;

    while i < left_slice.len() && j < right_slice.len() {
        if left_slice[i] <= right_slice[j] {
            arr[k] = left_slice[i];
            i += 1;
        } else {
            arr[k] = right_slice[j];
            j += 1;
        }
        k += 1;
    }

    
    while i < left_slice.len() {
        arr[k] = left_slice[i];
        k += 1;
        i += 1;
    }

    
    while j < right_slice.len() {
        arr[k] = right_slice[j];
        k += 1;
        j += 1;
    }
}








pub fn tim_sort<T: Ord + Copy>(arr: &mut [T]) {
    let n = arr.len();
    let min_run = compute_min_run_length(MIN_MERGE);

    
    let mut i = 0;
    while i < n {
        insertion_sort(&mut arr[i..cmp::min(i + MIN_MERGE, n)]);
        i += min_run;
    }

    
    let mut size = min_run;
    while size < n {
        let mut left = 0;
        while left < n {
            let mid = left + size - 1;
            let right = cmp::min(left + 2 * size - 1, n - 1);
            if mid < right {
                merge(arr, left, mid, right);
            }

            left += 2 * size;
        }
        size *= 2;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sorting::{have_same_elements, is_sorted};

    #[test]
    fn min_run_length_returns_correct_value() {
        assert_eq!(compute_min_run_length(0), 0);
        assert_eq!(compute_min_run_length(10), 10);
        assert_eq!(compute_min_run_length(33), 17);
        assert_eq!(compute_min_run_length(64), 16);
    }

    macro_rules! test_merge {
        ($($name:ident: $inputs:expr,)*) => {
            $(
                #[test]
                fn $name() {
                    let (input_arr, l, m, r, expected) = $inputs;
                    let mut arr = input_arr.clone();
                    merge(&mut arr, l, m, r);
                    assert_eq!(arr, expected);
                }
            )*
        }
    }

    test_merge! {
        left_and_right_subarrays_into_array: (vec![0, 2, 4, 1, 3, 5], 0, 2, 5, vec![0, 1, 2, 3, 4, 5]),
        with_empty_left_subarray: (vec![1, 2, 3], 0, 0, 2, vec![1, 2, 3]),
        with_empty_right_subarray: (vec![1, 2, 3], 0, 2, 2, vec![1, 2, 3]),
        with_empty_left_and_right_subarrays: (vec![1, 2, 3], 1, 0, 0, vec![1, 2, 3]),
    }

    macro_rules! test_tim_sort {
        ($($name:ident: $input:expr,)*) => {
            $(
                #[test]
                fn $name() {
                    let mut array = $input;
                    let cloned = array.clone();
                    tim_sort(&mut array);
                    assert!(is_sorted(&array) && have_same_elements(&array, &cloned));
                }
            )*
        }
    }

    test_tim_sort! {
        sorts_basic_array_correctly: vec![-2, 7, 15, -14, 0, 15, 0, 7, -7, -4, -13, 5, 8, -14, 12],
        sorts_long_array_correctly: vec![-2, 7, 15, -14, 0, 15, 0, 7, -7, -4, -13, 5, 8, -14, 12, 5, 3, 9, 22, 1, 1, 2, 3, 9, 6, 5, 4, 5, 6, 7, 8, 9, 1],
        handles_empty_array: Vec::<i32>::new(),
        handles_single_element_array: vec![3],
        handles_pre_sorted_array: vec![0, 1, 2, 3, 4, 5, 6, 7, 8, 9],
    }
}
