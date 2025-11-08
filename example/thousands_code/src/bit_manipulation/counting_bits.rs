














pub fn count_set_bits(mut n: usize) -> usize {
    
    let mut count = 0;
    while n > 0 {
        
        
        n &= n - 1;

        
        count += 1;
    }

    count
}

#[cfg(test)]
mod tests {
    use super::*;

    macro_rules! test_count_set_bits {
        ($($name:ident: $test_case:expr,)*) => {
            $(
                #[test]
                fn $name() {
                    let (input, expected) = $test_case;
                    assert_eq!(count_set_bits(input), expected);
                }
            )*
        };
    }
    test_count_set_bits! {
        test_count_set_bits_zero: (0, 0),
        test_count_set_bits_one: (1, 1),
        test_count_set_bits_power_of_two: (16, 1),
        test_count_set_bits_all_set_bits: (usize::MAX, std::mem::size_of::<usize>() * 8),
        test_count_set_bits_alternating_bits: (0b10101010, 4),
        test_count_set_bits_mixed_bits: (0b11011011, 6),
    }
}
