


















pub fn longest_common_substring(s1: &str, s2: &str) -> usize {
    let mut substr_len = vec![vec![0; s2.len() + 1]; s1.len() + 1];
    let mut max_len = 0;

    s1.as_bytes().iter().enumerate().for_each(|(i, &c1)| {
        s2.as_bytes().iter().enumerate().for_each(|(j, &c2)| {
            if c1 == c2 {
                substr_len[i + 1][j + 1] = substr_len[i][j] + 1;
                max_len = max_len.max(substr_len[i + 1][j + 1]);
            }
        });
    });

    max_len
}

#[cfg(test)]
mod tests {
    use super::*;

    macro_rules! test_longest_common_substring {
        ($($name:ident: $inputs:expr,)*) => {
            $(
                #[test]
                fn $name() {
                    let (s1, s2, expected) = $inputs;
                    assert_eq!(longest_common_substring(s1, s2), expected);
                    assert_eq!(longest_common_substring(s2, s1), expected);
                }
            )*
        }
    }

    test_longest_common_substring! {
        test_empty_strings: ("", "", 0),
        test_one_empty_string: ("", "a", 0),
        test_identical_single_char: ("a", "a", 1),
        test_different_single_char: ("a", "b", 0),
        test_common_substring_at_start: ("abcdef", "abc", 3),
        test_common_substring_at_middle: ("abcdef", "bcd", 3),
        test_common_substring_at_end: ("abcdef", "def", 3),
        test_no_common_substring: ("abc", "xyz", 0),
        test_overlapping_substrings: ("abcdxyz", "xyzabcd", 4),
        test_special_characters: ("@abc#def$", "#def@", 4),
        test_case_sensitive: ("abcDEF", "ABCdef", 0),
        test_full_string_match: ("GeeksforGeeks", "GeeksforGeeks", 13),
        test_substring_with_repeated_chars: ("aaaaaaaaaaaaa", "aaa", 3),
        test_longer_strings_with_common_substring: ("OldSite:GeeksforGeeks.org", "NewSite:GeeksQuiz.com", 10),
        test_no_common_substring_with_special_chars: ("!!!", "???", 0),
    }
}
