






pub fn signum(number: f64) -> i8 {
    if number == 0.0 {
        return 0;
    } else if number > 0.0 {
        return 1;
    }

    -1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn positive_integer() {
        assert_eq!(signum(15.0), 1);
    }

    #[test]
    fn negative_integer() {
        assert_eq!(signum(-30.0), -1);
    }

    #[test]
    fn zero() {
        assert_eq!(signum(0.0), 0);
    }
}
