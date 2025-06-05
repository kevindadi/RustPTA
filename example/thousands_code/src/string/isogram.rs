


use std::collections::HashMap;


#[derive(Debug, PartialEq, Eq)]
pub enum IsogramError {
    
    NonAlphabeticCharacter,
}



















fn count_letters(s: &str) -> Result<HashMap<char, usize>, IsogramError> {
    let mut letter_counts = HashMap::new();

    for ch in s.to_ascii_lowercase().chars() {
        if !ch.is_ascii_alphabetic() && !ch.is_whitespace() {
            return Err(IsogramError::NonAlphabeticCharacter);
        }

        if ch.is_ascii_alphabetic() {
            *letter_counts.entry(ch).or_insert(0) += 1;
        }
    }

    Ok(letter_counts)
}














pub fn is_isogram(s: &str) -> Result<bool, IsogramError> {
    let letter_counts = count_letters(s)?;
    Ok(letter_counts.values().all(|&count| count == 1))
}

#[cfg(test)]
mod tests {
    use super::*;

    macro_rules! isogram_tests {
        ($($name:ident: $tc:expr,)*) => {
            $(
                #[test]
                fn $name() {
                    let (input, expected) = $tc;
                    assert_eq!(is_isogram(input), expected);
                }
            )*
        };
    }

    isogram_tests! {
        isogram_simple: ("isogram", Ok(true)),
        isogram_case_insensitive: ("Isogram", Ok(true)),
        isogram_with_spaces: ("a b c d e", Ok(true)),
        isogram_mixed: ("Dermatoglyphics", Ok(true)),
        isogram_long: ("Subdermatoglyphic", Ok(true)),
        isogram_german_city: ("Malitzschkendorf", Ok(true)),
        perfect_pangram: ("Cwm fjord bank glyphs vext quiz", Ok(true)),
        isogram_sentences: ("The big dwarf only jumps", Ok(true)),
        isogram_french: ("Lampez un fort whisky", Ok(true)),
        isogram_portuguese: ("Velho traduz sim", Ok(true)),
        isogram_spanis: ("Centrifugadlos", Ok(true)),
        invalid_isogram_with_repeated_char: ("hello", Ok(false)),
        invalid_isogram_with_numbers: ("abc123", Err(IsogramError::NonAlphabeticCharacter)),
        invalid_isogram_with_special_char: ("abc!", Err(IsogramError::NonAlphabeticCharacter)),
        invalid_isogram_with_comma: ("Velho, traduz sim", Err(IsogramError::NonAlphabeticCharacter)),
        invalid_isogram_with_spaces: ("a b c d a", Ok(false)),
        invalid_isogram_with_repeated_phrase: ("abcabc", Ok(false)),
        isogram_empty_string: ("", Ok(true)),
        isogram_single_character: ("a", Ok(true)),
        invalid_isogram_multiple_same_characters: ("aaaa", Ok(false)),
        invalid_isogram_with_symbols: ("abc@#$%", Err(IsogramError::NonAlphabeticCharacter)),
    }
}
