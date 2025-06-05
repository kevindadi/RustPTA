extern crate num_bigint;
extern crate num_traits;

use num_bigint::BigInt;
use num_traits::FromPrimitive;




















pub fn binom(n: u64, k: u64) -> BigInt {
    let mut res = BigInt::from_u64(1).unwrap();
    for i in 0..k {
        res = (res * BigInt::from_u64(n - i).unwrap()) / BigInt::from_u64(i + 1).unwrap();
    }
    res
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_binom_5_2() {
        assert_eq!(binom(5, 2), BigInt::from(10));
    }

    #[test]
    fn test_binom_10_5() {
        assert_eq!(binom(10, 5), BigInt::from(252));
    }

    #[test]
    fn test_binom_0_0() {
        assert_eq!(binom(0, 0), BigInt::from(1));
    }

    #[test]
    fn test_binom_large_n_small_k() {
        assert_eq!(binom(1000, 2), BigInt::from(499500));
    }

    #[test]
    fn test_binom_random_1() {
        
        assert_eq!(binom(7, 4), BigInt::from(35));
    }

    #[test]
    fn test_binom_random_2() {
        
        assert_eq!(binom(12, 3), BigInt::from(220));
    }

    #[test]
    fn test_binom_random_3() {
        
        assert_eq!(binom(20, 10), BigInt::from(184_756));
    }
}
