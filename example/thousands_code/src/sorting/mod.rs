pub mod bead_sort;
pub mod binary_insertion_sort;
pub mod bingo_sort;
pub mod bitonic_sort;
pub mod bogo_sort;
pub mod bubble_sort;
pub mod bucket_sort;
pub mod cocktail_shaker_sort;
pub mod comb_sort;
pub mod counting_sort;
pub mod cycle_sort;
pub mod dutch_national_flag_sort;
pub mod exchange_sort;
pub mod gnome_sort;
pub mod heap_sort;
pub mod insertion_sort;
pub mod intro_sort;
pub mod merge_sort;
pub mod odd_even_sort;
pub mod pancake_sort;
pub mod patience_sort;
pub mod pigeonhole_sort;
pub mod quick_sort;
pub mod quick_sort_3_ways;
pub mod radix_sort;
pub mod selection_sort;
pub mod shell_sort;
pub mod sleep_sort;
#[cfg(test)]
pub mod sort_utils;
pub mod stooge_sort;
pub mod tim_sort;
pub mod tree_sort;
pub mod wave_sort;
pub mod wiggle_sort;

pub use self::bead_sort::bead_sort;
pub use self::binary_insertion_sort::binary_insertion_sort;
pub use self::bingo_sort::bingo_sort;
pub use self::bitonic_sort::bitonic_sort;
pub use self::bogo_sort::bogo_sort;
pub use self::bubble_sort::bubble_sort;
pub use self::bucket_sort::bucket_sort;
pub use self::cocktail_shaker_sort::cocktail_shaker_sort;
pub use self::comb_sort::comb_sort;
pub use self::counting_sort::counting_sort;
pub use self::counting_sort::generic_counting_sort;
pub use self::cycle_sort::cycle_sort;
pub use self::dutch_national_flag_sort::dutch_national_flag_sort;
pub use self::exchange_sort::exchange_sort;
pub use self::gnome_sort::gnome_sort;
pub use self::heap_sort::heap_sort;
pub use self::insertion_sort::insertion_sort;
pub use self::intro_sort::intro_sort;
pub use self::merge_sort::bottom_up_merge_sort;
pub use self::merge_sort::top_down_merge_sort;
pub use self::odd_even_sort::odd_even_sort;
pub use self::pancake_sort::pancake_sort;
pub use self::patience_sort::patience_sort;
pub use self::pigeonhole_sort::pigeonhole_sort;
pub use self::quick_sort::{partition, quick_sort};
pub use self::quick_sort_3_ways::quick_sort_3_ways;
pub use self::radix_sort::radix_sort;
pub use self::selection_sort::selection_sort;
pub use self::shell_sort::shell_sort;
pub use self::sleep_sort::sleep_sort;
pub use self::stooge_sort::stooge_sort;
pub use self::tim_sort::tim_sort;
pub use self::tree_sort::tree_sort;
pub use self::wave_sort::wave_sort;
pub use self::wiggle_sort::wiggle_sort;

#[cfg(test)]
use std::cmp;

#[cfg(test)]
pub fn have_same_elements<T>(a: &[T], b: &[T]) -> bool
where
    // T: cmp::PartialOrd,
    // If HashSet is used
    T: cmp::PartialOrd + cmp::Eq + std::hash::Hash,
{
    use std::collections::HashSet;

    if a.len() == b.len() {
        // This is O(n^2) but performs better on smaller data sizes
        //b.iter().all(|item| a.contains(item))

        // This is O(n), performs well on larger data sizes
        let set_a: HashSet<&T> = a.iter().collect();
        let set_b: HashSet<&T> = b.iter().collect();
        set_a == set_b
    } else {
        false
    }
}

#[cfg(test)]
pub fn is_sorted<T>(arr: &[T]) -> bool
where
    T: cmp::PartialOrd,
{
    arr.windows(2).all(|w| w[0] <= w[1])
}

#[cfg(test)]
pub fn is_descending_sorted<T>(arr: &[T]) -> bool
where
    T: cmp::PartialOrd,
{
    arr.windows(2).all(|w| w[0] >= w[1])
}

#[cfg(test)]
pub mod tests {
    #[test]
    fn is_sorted() {
        use super::*;

        assert!(is_sorted(&[] as &[isize]));
        assert!(is_sorted(&["a"]));
        assert!(is_sorted(&[1, 2, 3]));
        assert!(is_sorted(&[0, 1, 1]));

        assert!(!is_sorted(&[1, 0]));
        assert!(!is_sorted(&[2, 3, 1, -1, 5]));
    }
}
