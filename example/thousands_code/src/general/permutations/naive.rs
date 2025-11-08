use std::collections::HashSet;
use std::fmt::Debug;
use std::hash::Hash;


pub fn permute<T: Clone + Debug>(arr: &[T]) -> Vec<Vec<T>> {
    if arr.is_empty() {
        return vec![vec![]];
    }
    let n = arr.len();
    let count = (1..=n).product(); 
    let mut collector = Vec::with_capacity(count); 
    let mut arr = arr.to_owned(); 

    
    
    
    permute_recurse(&mut arr, n, &mut collector);
    collector
}

fn permute_recurse<T: Clone + Debug>(arr: &mut Vec<T>, k: usize, collector: &mut Vec<Vec<T>>) {
    if k == 1 {
        collector.push(arr.to_owned());
        return;
    }
    for i in 0..k {
        arr.swap(i, k - 1); 
        permute_recurse(arr, k - 1, collector); 
        arr.swap(i, k - 1); 
    }
}




pub fn permute_unique<T: Clone + Debug + Eq + Hash + Copy>(arr: &[T]) -> Vec<Vec<T>> {
    if arr.is_empty() {
        return vec![vec![]];
    }
    let n = arr.len();
    let count = (1..=n).product(); 
    let mut collector = Vec::with_capacity(count); 
    let mut arr = arr.to_owned(); 
    permute_recurse_unique(&mut arr, n, &mut collector);
    collector
}

fn permute_recurse_unique<T: Clone + Debug + Eq + Hash + Copy>(
    arr: &mut Vec<T>,
    k: usize,
    collector: &mut Vec<Vec<T>>,
) {
    
    if k == 1 {
        collector.push(arr.to_owned());
        return;
    }
    
    
    
    
    
    
    
    
    
    let mut swapped = HashSet::with_capacity(k);
    for i in 0..k {
        if swapped.contains(&arr[i]) {
            continue;
        }
        swapped.insert(arr[i]);
        arr.swap(i, k - 1); 
        permute_recurse_unique(arr, k - 1, collector); 
        arr.swap(i, k - 1); 
    }
}

#[cfg(test)]
mod tests {
    use crate::general::permutations::naive::{permute, permute_unique};
    use crate::general::permutations::tests::{
        assert_permutations, assert_valid_permutation, NotTooBigVec,
    };
    use quickcheck_macros::quickcheck;
    use std::collections::HashSet;

    #[test]
    fn test_3_different_values() {
        let original = vec![1, 2, 3];
        let res = permute(&original);
        assert_eq!(res.len(), 6); 
        for permut in res {
            assert_valid_permutation(&original, &permut)
        }
    }

    #[test]
    fn empty_array() {
        let empty: std::vec::Vec<u8> = vec![];
        assert_eq!(permute(&empty), vec![vec![]]);
        assert_eq!(permute_unique(&empty), vec![vec![]]);
    }

    #[test]
    fn test_3_times_the_same_value() {
        let original = vec![1, 1, 1];
        let res = permute(&original);
        assert_eq!(res.len(), 6); 
        for permut in res {
            assert_valid_permutation(&original, &permut)
        }
    }

    #[quickcheck]
    fn test_some_elements(NotTooBigVec { inner: original }: NotTooBigVec) {
        let permutations = permute(&original);
        assert_permutations(&original, &permutations)
    }

    #[test]
    fn test_unique_values() {
        let original = vec![1, 1, 2, 2];
        let unique_permutations = permute_unique(&original);
        let every_permutation = permute(&original);
        for unique_permutation in &unique_permutations {
            assert!(every_permutation.contains(unique_permutation));
        }
        assert_eq!(
            unique_permutations.len(),
            every_permutation.iter().collect::<HashSet<_>>().len()
        )
    }
}
