






















pub fn get_nth_frizzy(base: i32, mut n: i32) -> f64 {
    let mut final1 = 0.0;
    let mut i = 0;
    while n > 0 {
        final1 += (base.pow(i) as f64) * ((n % 2) as f64);
        i += 1;
        n /= 2;
    }
    final1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_nth_frizzy() {
        
        
        assert_eq!(get_nth_frizzy(3, 4), 9.0);

        
        
        assert_eq!(get_nth_frizzy(2, 5), 5.0);

        
        
        assert_eq!(get_nth_frizzy(4, 3), 5.0);

        
        
        assert_eq!(get_nth_frizzy(5, 2), 5.0);

        
        
        assert_eq!(get_nth_frizzy(6, 1), 1.0);
    }
}
