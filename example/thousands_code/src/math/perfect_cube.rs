
pub fn perfect_cube_binary_search(n: i64) -> bool {
    if n < 0 {
        return perfect_cube_binary_search(-n);
    }

    
    let mut left = 0;
    let mut right = n.abs(); 

    
    while left <= right {
        
        let mid = left + (right - left) / 2;
        
        let cube = mid * mid * mid;

        
        match cube.cmp(&n) {
            std::cmp::Ordering::Equal => return true,
            std::cmp::Ordering::Less => left = mid + 1,
            std::cmp::Ordering::Greater => right = mid - 1,
        }
    }

    
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    macro_rules! test_perfect_cube {
        ($($name:ident: $inputs:expr,)*) => {
        $(
            #[test]
            fn $name() {
                let (n, expected) = $inputs;
                assert_eq!(perfect_cube_binary_search(n), expected);
                assert_eq!(perfect_cube_binary_search(-n), expected);
            }
        )*
        }
    }

    test_perfect_cube! {
        num_0_a_perfect_cube: (0, true),
        num_1_is_a_perfect_cube: (1, true),
        num_27_is_a_perfect_cube: (27, true),
        num_64_is_a_perfect_cube: (64, true),
        num_8_is_a_perfect_cube: (8, true),
        num_2_is_not_a_perfect_cube: (2, false),
        num_3_is_not_a_perfect_cube: (3, false),
        num_4_is_not_a_perfect_cube: (4, false),
        num_5_is_not_a_perfect_cube: (5, false),
        num_999_is_not_a_perfect_cube: (999, false),
        num_1000_is_a_perfect_cube: (1000, true),
        num_1001_is_not_a_perfect_cube: (1001, false),
    }
}
