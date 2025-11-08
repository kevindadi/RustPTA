




pub fn sylvester(number: i32) -> i128 {
    assert!(number > 0, "The input value of [n={number}] has to be > 0");

    if number == 1 {
        2
    } else {
        let num = sylvester(number - 1);
        let lower = num - 1;
        let upper = num;
        lower * upper + 1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sylvester() {
        assert_eq!(sylvester(8), 113423713055421844361000443_i128);
    }

    #[test]
    #[should_panic(expected = "The input value of [n=-1] has to be > 0")]
    fn test_sylvester_negative() {
        sylvester(-1);
    }
}
