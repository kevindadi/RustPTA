













pub fn sum_digits_iterative(num: i32) -> u32 {
    
    let mut num = num.unsigned_abs();
    
    let mut result: u32 = 0;

    
    while num > 0 {
        
        result += num % 10;
        num /= 10; 
    }
    result
}















pub fn sum_digits_recursive(num: i32) -> u32 {
    
    let num = num.unsigned_abs();
    
    if num < 10 {
        return num;
    }
    
    num % 10 + sum_digits_recursive((num / 10) as i32)
}

#[cfg(test)]
mod tests {
    mod iterative {
        
        use super::super::sum_digits_iterative as sum_digits;

        #[test]
        fn zero() {
            assert_eq!(0, sum_digits(0));
        }
        #[test]
        fn positive_number() {
            assert_eq!(1, sum_digits(1));
            assert_eq!(10, sum_digits(1234));
            assert_eq!(14, sum_digits(42161));
            assert_eq!(6, sum_digits(500010));
        }
        #[test]
        fn negative_number() {
            assert_eq!(1, sum_digits(-1));
            assert_eq!(12, sum_digits(-246));
            assert_eq!(2, sum_digits(-11));
            assert_eq!(14, sum_digits(-42161));
            assert_eq!(6, sum_digits(-500010));
        }
        #[test]
        fn trailing_zeros() {
            assert_eq!(1, sum_digits(1000000000));
            assert_eq!(3, sum_digits(300));
        }
    }

    mod recursive {
        
        use super::super::sum_digits_recursive as sum_digits;

        #[test]
        fn zero() {
            assert_eq!(0, sum_digits(0));
        }
        #[test]
        fn positive_number() {
            assert_eq!(1, sum_digits(1));
            assert_eq!(10, sum_digits(1234));
            assert_eq!(14, sum_digits(42161));
            assert_eq!(6, sum_digits(500010));
        }
        #[test]
        fn negative_number() {
            assert_eq!(1, sum_digits(-1));
            assert_eq!(12, sum_digits(-246));
            assert_eq!(2, sum_digits(-11));
            assert_eq!(14, sum_digits(-42161));
            assert_eq!(6, sum_digits(-500010));
        }
        #[test]
        fn trailing_zeros() {
            assert_eq!(1, sum_digits(1000000000));
            assert_eq!(3, sum_digits(300));
        }
    }
}
