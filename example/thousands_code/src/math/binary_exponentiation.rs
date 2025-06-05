











pub fn binary_exponentiation(mut n: u64, mut p: u32) -> u64 {
    let mut result_pow: u64 = 1;
    while p > 0 {
        if p & 1 == 1 {
            result_pow *= n;
        }
        p >>= 1;
        n *= n;
    }
    result_pow
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic() {
        
        assert_eq!(binary_exponentiation(2, 3), 8);
        assert_eq!(binary_exponentiation(4, 12), 16777216);
        assert_eq!(binary_exponentiation(6, 12), 2176782336);
        assert_eq!(binary_exponentiation(10, 4), 10000);
        assert_eq!(binary_exponentiation(20, 3), 8000);
        assert_eq!(binary_exponentiation(3, 21), 10460353203);
    }

    #[test]
    fn up_to_ten() {
        
        for i in 0..10 {
            for j in 0..10 {
                println!("{i}, {j}");
                assert_eq!(binary_exponentiation(i, j), u64::pow(i, j))
            }
        }
    }
}
