






pub fn radix_sort(arr: &mut [u64]) {
    let max: usize = match arr.iter().max() {
        Some(&x) => x as usize,
        None => return,
    };
    
    let radix = arr.len().next_power_of_two();
    
    let mut place = 1;
    while place <= max {
        let digit_of = |x| x as usize / place % radix;
        
        let mut counter = vec![0; radix];
        for &x in arr.iter() {
            counter[digit_of(x)] += 1;
        }
        
        for i in 1..radix {
            counter[i] += counter[i - 1];
        }
        
        for &x in arr.to_owned().iter().rev() {
            counter[digit_of(x)] -= 1;
            arr[counter[digit_of(x)]] = x;
        }
        place *= radix;
    }
}

#[cfg(test)]
mod tests {
    use super::radix_sort;
    use crate::sorting::have_same_elements;
    use crate::sorting::is_sorted;

    #[test]
    fn empty() {
        let mut a: [u64; 0] = [];
        let cloned = a;
        radix_sort(&mut a);
        assert!(is_sorted(&a) && have_same_elements(&a, &cloned));
    }

    #[test]
    fn descending() {
        let mut v = vec![201, 127, 64, 37, 24, 4, 1];
        let cloned = v.clone();
        radix_sort(&mut v);
        assert!(is_sorted(&v) && have_same_elements(&v, &cloned));
    }

    #[test]
    fn ascending() {
        let mut v = vec![1, 4, 24, 37, 64, 127, 201];
        let cloned = v.clone();
        radix_sort(&mut v);
        assert!(is_sorted(&v) && have_same_elements(&v, &cloned));
    }
}
