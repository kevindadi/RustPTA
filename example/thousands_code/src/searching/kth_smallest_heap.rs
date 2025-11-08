use crate::data_structures::Heap;
use std::cmp::{Ord, Ordering};










pub fn kth_smallest_heap<T>(input: &[T], k: usize) -> Option<T>
where
    T: Ord + Copy,
{
    if input.len() < k {
        return None;
    }

    
    
    
    
    
    
    
    
    
    
    let mut heap = Heap::new_max();

    
    for &val in input.iter().take(k) {
        heap.add(val);
    }

    for &val in input.iter().skip(k) {
        
        let cur_big = heap.pop().unwrap(); 
        match val.cmp(&cur_big) {
            Ordering::Greater => {
                heap.add(cur_big);
            }
            _ => {
                heap.add(val);
            }
        }
    }

    heap.pop()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty() {
        let zero: [u8; 0] = [];
        let first = kth_smallest_heap(&zero, 1);

        assert_eq!(None, first);
    }

    #[test]
    fn one_element() {
        let one = [1];
        let first = kth_smallest_heap(&one, 1);

        assert_eq!(1, first.unwrap());
    }

    #[test]
    fn many_elements() {
        
        let many = [9, 17, 3, 16, 13, 10, 1, 5, 7, 12, 4, 8, 9, 0];

        let first = kth_smallest_heap(&many, 1);
        let third = kth_smallest_heap(&many, 3);
        let sixth = kth_smallest_heap(&many, 6);
        let fourteenth = kth_smallest_heap(&many, 14);

        assert_eq!(0, first.unwrap());
        assert_eq!(3, third.unwrap());
        assert_eq!(7, sixth.unwrap());
        assert_eq!(17, fourteenth.unwrap());
    }
}
