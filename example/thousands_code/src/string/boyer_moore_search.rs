




use std::collections::HashMap;










fn build_bad_char_table(pat: &[char]) -> HashMap<char, isize> {
    let mut bad_char_table = HashMap::new();
    for (i, &ch) in pat.iter().enumerate() {
        bad_char_table.insert(ch, i as isize);
    }
    bad_char_table
}













fn calc_match_shift(
    shift: isize,
    pat_len: isize,
    text_len: isize,
    bad_char_table: &HashMap<char, isize>,
    text: &[char],
) -> isize {
    if shift + pat_len >= text_len {
        return 1;
    }
    let next_ch = text[(shift + pat_len) as usize];
    pat_len - bad_char_table.get(&next_ch).unwrap_or(&-1)
}












fn calc_mismatch_shift(
    mis_idx: isize,
    shift: isize,
    text: &[char],
    bad_char_table: &HashMap<char, isize>,
) -> isize {
    let mis_ch = text[(shift + mis_idx) as usize];
    let bad_char_shift = bad_char_table.get(&mis_ch).unwrap_or(&-1);
    std::cmp::max(1, mis_idx - bad_char_shift)
}













pub fn boyer_moore_search(text: &str, pat: &str) -> Vec<usize> {
    let mut positions = Vec::new();

    let text_len = text.len() as isize;
    let pat_len = pat.len() as isize;

    
    if text_len == 0 || pat_len == 0 || pat_len > text_len {
        return positions;
    }

    
    let pat: Vec<char> = pat.chars().collect();
    let text: Vec<char> = text.chars().collect();

    
    let bad_char_table = build_bad_char_table(&pat);

    let mut shift = 0;

    
    while shift <= text_len - pat_len {
        let mut j = pat_len - 1;

        
        while j >= 0 && pat[j as usize] == text[(shift + j) as usize] {
            j -= 1;
        }

        
        if j < 0 {
            positions.push(shift as usize);
            shift += calc_match_shift(shift, pat_len, text_len, &bad_char_table, &text);
        } else {
            
            shift += calc_mismatch_shift(j, shift, &text, &bad_char_table);
        }
    }

    positions
}

#[cfg(test)]
mod tests {
    use super::*;

    macro_rules! boyer_moore_tests {
        ($($name:ident: $tc:expr,)*) => {
            $(
                #[test]
                fn $name() {
                    let (text, pattern, expected) = $tc;
                    assert_eq!(boyer_moore_search(text, pattern), expected);
                }
            )*
        };
    }

    boyer_moore_tests! {
        test_simple_match: ("AABCAB12AFAABCABFFEGABCAB", "ABCAB", vec![1, 11, 20]),
        test_no_match: ("AABCAB12AFAABCABFFEGABCAB", "FFF", vec![]),
        test_partial_match: ("AABCAB12AFAABCABFFEGABCAB", "CAB", vec![3, 13, 22]),
        test_empty_text: ("", "A", vec![]),
        test_empty_pattern: ("ABC", "", vec![]),
        test_both_empty: ("", "", vec![]),
        test_pattern_longer_than_text: ("ABC", "ABCDEFG", vec![]),
        test_single_character_text: ("A", "A", vec![0]),
        test_single_character_pattern: ("AAAA", "A", vec![0, 1, 2, 3]),
        test_case_sensitivity: ("ABCabcABC", "abc", vec![3]),
        test_overlapping_patterns: ("AAAAA", "AAA", vec![0, 1, 2]),
        test_special_characters: ("@!#$$%^&*", "$$", vec![3]),
        test_numerical_pattern: ("123456789123456", "456", vec![3, 12]),
        test_partial_overlap_no_match: ("ABCD", "ABCDE", vec![]),
        test_single_occurrence: ("XXXXXXXXXXXXXXXXXXPATTERNXXXXXXXXXXXXXXXXXX", "PATTERN", vec![18]),
        test_single_occurrence_with_noise: ("PATPATPATPATTERNPAT", "PATTERN", vec![9]),
    }
}
