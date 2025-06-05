




use std::cmp::min;



























pub fn naive_levenshtein_distance(string1: &str, string2: &str) -> usize {
    let distance_matrix: Vec<Vec<usize>> = (0..=string1.len())
        .map(|i| {
            (0..=string2.len())
                .map(|j| {
                    if i == 0 {
                        j
                    } else if j == 0 {
                        i
                    } else {
                        0
                    }
                })
                .collect()
        })
        .collect();

    let updated_matrix = (1..=string1.len()).fold(distance_matrix, |matrix, i| {
        (1..=string2.len()).fold(matrix, |mut inner_matrix, j| {
            let cost = if string1.as_bytes()[i - 1] == string2.as_bytes()[j - 1] {
                0
            } else {
                1
            };
            inner_matrix[i][j] = (inner_matrix[i - 1][j - 1] + cost)
                .min(inner_matrix[i][j - 1] + 1)
                .min(inner_matrix[i - 1][j] + 1);
            inner_matrix
        })
    });

    updated_matrix[string1.len()][string2.len()]
}


























pub fn optimized_levenshtein_distance(string1: &str, string2: &str) -> usize {
    if string1.is_empty() {
        return string2.len();
    }
    let l1 = string1.len();
    let mut prev_dist: Vec<usize> = (0..=l1).collect();

    for (row, c2) in string2.chars().enumerate() {
        
        let mut prev_substitution_cost = prev_dist[0];
        
        prev_dist[0] = row + 1;

        for (col, c1) in string1.chars().enumerate() {
            
            let deletion_cost = prev_dist[col] + 1;
            
            let insertion_cost = prev_dist[col + 1] + 1;
            let substitution_cost = if c1 == c2 {
                
                prev_substitution_cost
            } else {
                
                prev_substitution_cost + 1
            };
            
            prev_substitution_cost = prev_dist[col + 1];
            prev_dist[col + 1] = _min3(deletion_cost, insertion_cost, substitution_cost);
        }
    }
    prev_dist[l1]
}

#[inline]
fn _min3<T: Ord>(a: T, b: T, c: T) -> T {
    min(a, min(b, c))
}

#[cfg(test)]
mod tests {
    const LEVENSHTEIN_DISTANCE_TEST_CASES: &[(&str, &str, usize)] = &[
        ("", "", 0),
        ("Hello, World!", "Hello, World!", 0),
        ("", "Rust", 4),
        ("horse", "ros", 3),
        ("tan", "elephant", 6),
        ("execute", "intention", 8),
    ];

    macro_rules! levenshtein_distance_tests {
        ($function:ident) => {
            mod $function {
                use super::*;

                fn run_test_case(string1: &str, string2: &str, expected_distance: usize) {
                    assert_eq!(super::super::$function(string1, string2), expected_distance);
                    assert_eq!(super::super::$function(string2, string1), expected_distance);
                    assert_eq!(super::super::$function(string1, string1), 0);
                    assert_eq!(super::super::$function(string2, string2), 0);
                }

                #[test]
                fn test_levenshtein_distance() {
                    for &(string1, string2, expected_distance) in
                        LEVENSHTEIN_DISTANCE_TEST_CASES.iter()
                    {
                        run_test_case(string1, string2, expected_distance);
                    }
                }
            }
        };
    }

    levenshtein_distance_tests!(naive_levenshtein_distance);
    levenshtein_distance_tests!(optimized_levenshtein_distance);
}
