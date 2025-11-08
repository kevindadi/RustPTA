const ERROR_MESSAGE: &str = "Rotation must be in the range [0, 25]";
const ALPHABET_LENGTH: u8 = b'z' - b'a' + 1;





















pub fn caesar(text: &str, rotation: isize) -> Result<String, &'static str> {
    if !(0..ALPHABET_LENGTH as isize).contains(&rotation) {
        return Err(ERROR_MESSAGE);
    }

    let result = text
        .chars()
        .map(|c| {
            if c.is_ascii_alphabetic() {
                shift_char(c, rotation)
            } else {
                c
            }
        })
        .collect();

    Ok(result)
}











fn shift_char(c: char, rotation: isize) -> char {
    let first = if c.is_ascii_lowercase() { b'a' } else { b'A' };
    let rotation = rotation as u8; 

    (((c as u8 - first) + rotation) % ALPHABET_LENGTH + first) as char
}

#[cfg(test)]
mod tests {
    use super::*;

    macro_rules! test_caesar_happy_path {
        ($($name:ident: $test_case:expr,)*) => {
            $(
                #[test]
                fn $name() {
                    let (text, rotation, expected) = $test_case;
                    assert_eq!(caesar(&text, rotation).unwrap(), expected);

                    let backward_rotation = if rotation == 0 { 0 } else { ALPHABET_LENGTH as isize - rotation };
                    assert_eq!(caesar(&expected, backward_rotation).unwrap(), text);
                }
            )*
        };
    }

    macro_rules! test_caesar_error_cases {
        ($($name:ident: $test_case:expr,)*) => {
            $(
                #[test]
                fn $name() {
                    let (text, rotation) = $test_case;
                    assert_eq!(caesar(&text, rotation), Err(ERROR_MESSAGE));
                }
            )*
        };
    }

    #[test]
    fn alphabet_length_should_be_26() {
        assert_eq!(ALPHABET_LENGTH, 26);
    }

    test_caesar_happy_path! {
        empty_text: ("", 13, ""),
        rot_13: ("rust", 13, "ehfg"),
        unicode: ("attack at dawn 攻", 5, "fyyfhp fy ifbs 攻"),
        rotation_within_alphabet_range: ("Hello, World!", 3, "Khoor, Zruog!"),
        no_rotation: ("Hello, World!", 0, "Hello, World!"),
        rotation_at_alphabet_end: ("Hello, World!", 25, "Gdkkn, Vnqkc!"),
        longer: ("The quick brown fox jumps over the lazy dog.", 5, "Ymj vznhp gwtbs ktc ozrux tajw ymj qfed itl."),
        non_alphabetic_characters: ("12345!@#$%", 3, "12345!@#$%"),
        uppercase_letters: ("ABCDEFGHIJKLMNOPQRSTUVWXYZ", 1, "BCDEFGHIJKLMNOPQRSTUVWXYZA"),
        mixed_case: ("HeLlO WoRlD", 7, "OlSsV DvYsK"),
        with_whitespace: ("Hello, World!", 13, "Uryyb, Jbeyq!"),
        with_special_characters: ("Hello!@#$%^&*()_+World", 4, "Lipps!@#$%^&*()_+Asvph"),
        with_numbers: ("Abcd1234XYZ", 10, "Klmn1234HIJ"),
    }

    test_caesar_error_cases! {
        negative_rotation: ("Hello, World!", -5),
        empty_input_negative_rotation: ("", -1),
        empty_input_large_rotation: ("", 27),
        large_rotation: ("Large rotation", 139),
    }
}
