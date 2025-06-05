












pub fn gcd_extended(a: i64, m: i64) -> (i64, i64, i64) {
    if a == 0 {
        (m, 0, 1)
    } else {
        let (gcd, x1, x2) = gcd_extended(m % a, a);
        let x = x2 - (m / a) * x1;
        (gcd, x, x1)
    }
}















pub fn mod_inverse(b: i64, m: i64) -> i64 {
    let (gcd, x, _) = gcd_extended(b, m);
    if gcd != 1 {
        panic!("Inverse does not exist");
    } else {
        
        (x % m + m) % m
    }
}













pub fn modular_exponential(base: i64, mut power: i64, modulus: i64) -> i64 {
    if modulus == 1 {
        return 0; 
    }

    
    let mut base = if power < 0 {
        mod_inverse(base, modulus)
    } else {
        base % modulus
    };

    let mut result = 1; 
    power = power.abs(); 

    
    while power > 0 {
        if power & 1 == 1 {
            result = (result * base) % modulus;
        }
        power >>= 1; 
        base = (base * base) % modulus; 
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_modular_exponential_positive() {
        assert_eq!(modular_exponential(2, 3, 5), 3); 
        assert_eq!(modular_exponential(7, 2, 13), 10); 
        assert_eq!(modular_exponential(5, 5, 31), 25); 
        assert_eq!(modular_exponential(10, 8, 11), 1); 
        assert_eq!(modular_exponential(123, 45, 67), 62); 
    }

    #[test]
    fn test_modular_inverse() {
        assert_eq!(mod_inverse(7, 13), 2); 
        assert_eq!(mod_inverse(5, 31), 25); 
        assert_eq!(mod_inverse(10, 11), 10); 
        assert_eq!(mod_inverse(123, 67), 6); 
        assert_eq!(mod_inverse(9, 17), 2); 
    }

    #[test]
    fn test_modular_exponential_negative() {
        assert_eq!(
            modular_exponential(7, -2, 13),
            mod_inverse(7, 13).pow(2) % 13
        ); 
        assert_eq!(
            modular_exponential(5, -5, 31),
            mod_inverse(5, 31).pow(5) % 31
        ); 
        assert_eq!(
            modular_exponential(10, -8, 11),
            mod_inverse(10, 11).pow(8) % 11
        ); 
        assert_eq!(
            modular_exponential(123, -5, 67),
            mod_inverse(123, 67).pow(5) % 67
        ); 
    }

    #[test]
    fn test_modular_exponential_edge_cases() {
        assert_eq!(modular_exponential(0, 0, 1), 0); 
        assert_eq!(modular_exponential(0, 10, 1), 0); 
        assert_eq!(modular_exponential(10, 0, 1), 0); 
        assert_eq!(modular_exponential(1, 1, 1), 0); 
        assert_eq!(modular_exponential(-1, 2, 1), 0); 
    }
}
