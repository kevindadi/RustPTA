use crate::data_structures::Trie;










pub fn word_break(s: &str, word_dict: &[&str]) -> bool {
    let mut trie = Trie::new();
    for &word in word_dict {
        trie.insert(word.chars(), true);
    }

    
    let mut memo = vec![None; s.len() + 1];
    search(&trie, s, 0, &mut memo)
}












fn search(trie: &Trie<char, bool>, s: &str, start: usize, memo: &mut Vec<Option<bool>>) -> bool {
    if start == s.len() {
        return true;
    }

    if let Some(res) = memo[start] {
        return res;
    }

    for end in start + 1..=s.len() {
        if trie.get(s[start..end].chars()).is_some() && search(trie, s, end, memo) {
            memo[start] = Some(true);
            return true;
        }
    }

    memo[start] = Some(false);
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    macro_rules! test_cases {
        ($($name:ident: $test_case:expr,)*) => {
            $(
                #[test]
                fn $name() {
                    let (input, dict, expected) = $test_case;
                    assert_eq!(word_break(input, &dict), expected);
                }
            )*
        }
    }

    test_cases! {
        typical_case_1: ("applepenapple", vec!["apple", "pen"], true),
        typical_case_2: ("catsandog", vec!["cats", "dog", "sand", "and", "cat"], false),
        typical_case_3: ("cars", vec!["car", "ca", "rs"], true),
        edge_case_empty_string: ("", vec!["apple", "pen"], true),
        edge_case_empty_dict: ("apple", vec![], false),
        edge_case_single_char_in_dict: ("a", vec!["a"], true),
        edge_case_single_char_not_in_dict: ("b", vec!["a"], false),
        edge_case_all_words_larger_than_input: ("a", vec!["apple", "banana"], false),
        edge_case_no_solution_large_string: ("abcdefghijklmnoqrstuv", vec!["a", "bc", "def", "ghij", "klmno", "pqrst"], false),
        successful_segmentation_large_string: ("abcdefghijklmnopqrst", vec!["a", "bc", "def", "ghij", "klmno", "pqrst"], true),
        long_string_repeated_pattern: (&"ab".repeat(100), vec!["a", "b", "ab"], true),
        long_string_no_solution: (&"a".repeat(100), vec!["b"], false),
        mixed_size_dict_1: ("pineapplepenapple", vec!["apple", "pen", "applepen", "pine", "pineapple"], true),
        mixed_size_dict_2: ("catsandog", vec!["cats", "dog", "sand", "and", "cat"], false),
        mixed_size_dict_3: ("abcd", vec!["a", "abc", "b", "cd"], true),
        performance_stress_test_large_valid: (&"abc".repeat(1000), vec!["a", "ab", "abc"], true),
        performance_stress_test_large_invalid: (&"x".repeat(1000), vec!["a", "ab", "abc"], false),
    }
}
