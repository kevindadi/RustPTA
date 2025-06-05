




pub fn factors(number: u64) -> Vec<u64> {
    let mut factors: Vec<u64> = Vec::new();

    for i in 1..((number as f64).sqrt() as u64 + 1) {
        if number % i == 0 {
            factors.push(i);
            if i != number / i {
                factors.push(number / i);
            }
        }
    }

    factors.sort();
    factors
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prime_number() {
        assert_eq!(vec![1, 59], factors(59));
    }

    #[test]
    fn highly_composite_number() {
        assert_eq!(
            vec![
                1, 2, 3, 4, 5, 6, 8, 9, 10, 12, 15, 18, 20, 24, 30, 36, 40, 45, 60, 72, 90, 120,
                180, 360
            ],
            factors(360)
        );
    }

    #[test]
    fn composite_number() {
        assert_eq!(vec![1, 3, 23, 69], factors(69));
    }
}
