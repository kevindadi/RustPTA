





pub fn amicable_pairs_under_n(n: u32) -> Option<Vec<(u32, u32)>> {
    let mut factor_sums = vec![0; n as usize];

    
    for i in 1..n {
        for j in (i * 2..n).step_by(i as usize) {
            factor_sums[j as usize] += i;
        }
    }

    
    let mut out = vec![(0, 0)];
    
    for (i, x) in factor_sums.iter().enumerate() {
        if (*x < n) && (factor_sums[*x as usize] == i as u32) && (*x > i as u32) {
            out.push((i as u32, *x));
        }
    }

    
    if out.len() == 1 {
        None
    } else {
        out.remove(0);
        Some(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    pub fn test_amicable_numbers_below_n() {
        
        let expected_result = vec![
            (220, 284),
            (1184, 1210),
            (2620, 2924),
            (5020, 5564),
            (6232, 6368),
            (10744, 10856),
            (12285, 14595),
            (17296, 18416),
            (63020, 76084),
            (66928, 66992),
        ];

        
        let mut result = amicable_pairs_under_n(100_000).unwrap();

        
        assert_eq!(result.len(), 13);

        
        result = result[..10].to_vec();
        assert_eq!(result, expected_result);

        
        assert_eq!(amicable_pairs_under_n(100), None);
    }
}
