

















pub fn shortest_palindrome(s: &str) -> String {
    if s.is_empty() {
        return "".to_string();
    }

    let original_chars: Vec<char> = s.chars().collect();
    let suffix_table = compute_suffix(&original_chars);

    let mut reversed_chars: Vec<char> = s.chars().rev().collect();
    
    let prefix_match = compute_prefix_match(&original_chars, &reversed_chars, &suffix_table);

    reversed_chars.append(&mut original_chars[prefix_match[original_chars.len() - 1]..].to_vec());
    reversed_chars.iter().collect()
}













pub fn compute_suffix(chars: &[char]) -> Vec<usize> {
    let mut suffix = vec![0; chars.len()];
    for i in 1..chars.len() {
        let mut j = suffix[i - 1];
        while j > 0 && chars[j] != chars[i] {
            j = suffix[j - 1];
        }
        suffix[i] = j + if chars[j] == chars[i] { 1 } else { 0 };
    }
    suffix
}















pub fn compute_prefix_match(original: &[char], reversed: &[char], suffix: &[usize]) -> Vec<usize> {
    let mut match_table = vec![0; original.len()];
    match_table[0] = if original[0] == reversed[0] { 1 } else { 0 };
    for i in 1..original.len() {
        let mut j = match_table[i - 1];
        while j > 0 && reversed[i] != original[j] {
            j = suffix[j - 1];
        }
        match_table[i] = j + if reversed[i] == original[j] { 1 } else { 0 };
    }
    match_table
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::string::is_palindrome;

    macro_rules! test_shortest_palindrome {
        ($($name:ident: $inputs:expr,)*) => {
            $(
                #[test]
                fn $name() {
                    let (input, expected) = $inputs;
                    assert!(is_palindrome(expected));
                    assert_eq!(shortest_palindrome(input), expected);
                    assert_eq!(shortest_palindrome(expected), expected);
                }
            )*
        }
    }

    test_shortest_palindrome! {
        empty: ("", ""),
        extend_left_1: ("aacecaaa", "aaacecaaa"),
        extend_left_2: ("abcd", "dcbabcd"),
        unicode_1: ("അ", "അ"),
        unicode_2: ("a牛", "牛a牛"),
        single_char: ("x", "x"),
        already_palindrome: ("racecar", "racecar"),
        extend_left_3: ("abcde", "edcbabcde"),
        extend_left_4: ("abca", "acbabca"),
        long_string: ("abcdefg", "gfedcbabcdefg"),
        repetitive: ("aaaaa", "aaaaa"),
        complex: ("abacdfgdcaba", "abacdgfdcabacdfgdcaba"),
    }
}
