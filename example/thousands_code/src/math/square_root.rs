

pub fn square_root(num: f64) -> f64 {
    if num < 0.0_f64 {
        return f64::NAN;
    }

    let mut root = 1.0_f64;

    while (root * root - num).abs() > 1e-10_f64 {
        root -= (root * root - num) / (2.0_f64 * root);
    }

    root
}




pub fn fast_inv_sqrt(num: f32) -> f32 {
    
    if num < 0.0f32 {
        return f32::NAN;
    }

    let i = num.to_bits();
    let i = 0x5f3759df - (i >> 1);
    let y = f32::from_bits(i);

    println!("num: {:?}, out: {:?}", num, y * (1.5 - 0.5 * num * y * y));
    
    y * (1.5 - 0.5 * num * y * y)
    
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fast_inv_sqrt() {
        
        assert!(fast_inv_sqrt(-1.0f32).is_nan());

        
        let test_pairs = [(4.0, 0.5), (16.0, 0.25), (25.0, 0.2)];
        for pair in test_pairs {
            assert!((fast_inv_sqrt(pair.0) - pair.1).abs() <= (0.01 * pair.0));
        }
    }

    #[test]
    fn test_sqare_root() {
        assert!((square_root(4.0_f64) - 2.0_f64).abs() <= 1e-10_f64);
        assert!(square_root(-4.0_f64).is_nan());
    }
}
