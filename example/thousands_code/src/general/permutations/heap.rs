use std::fmt::Debug;



pub fn heap_permute<T: Clone + Debug>(arr: &[T]) -> Vec<Vec<T>> {
    if arr.is_empty() {
        return vec![vec![]];
    }
    let n = arr.len();
    let mut collector = Vec::with_capacity((1..=n).product()); 
    let mut arr = arr.to_owned(); 
    heap_recurse(&mut arr, n, &mut collector);
    collector
}

fn heap_recurse<T: Clone + Debug>(arr: &mut [T], k: usize, collector: &mut Vec<Vec<T>>) {
    if k == 1 {
        
        collector.push((*arr).to_owned());
        return;
    }
    
    
    for i in 0..k {
        
        let swap_idx = if k % 2 == 0 { i } else { 0 };
        arr.swap(swap_idx, k - 1);
        heap_recurse(arr, k - 1, collector);
    }
}

#[cfg(test)]
mod tests {
    use quickcheck_macros::quickcheck;

    use crate::general::permutations::heap_permute;
    use crate::general::permutations::tests::{
        assert_permutations, assert_valid_permutation, NotTooBigVec,
    };

    #[test]
    fn test_3_different_values() {
        let original = vec![1, 2, 3];
        let res = heap_permute(&original);
        assert_eq!(res.len(), 6); 
        for permut in res {
            assert_valid_permutation(&original, &permut)
        }
    }

    #[test]
    fn test_3_times_the_same_value() {
        let original = vec![1, 1, 1];
        let res = heap_permute(&original);
        assert_eq!(res.len(), 6); 
        for permut in res {
            assert_valid_permutation(&original, &permut)
        }
    }

    #[quickcheck]
    fn test_some_elements(NotTooBigVec { inner: original }: NotTooBigVec) {
        let permutations = heap_permute(&original);
        assert_permutations(&original, &permutations)
    }
}
