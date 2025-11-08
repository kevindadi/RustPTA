






pub fn baconian_encode(message: &str) -> String {
    let alphabet = "ABCDEFGHIJKLMNOPQRSTUVWXYZ";
    let baconian = [
        "AAAAA", "AAAAB", "AAABA", "AAABB", "AABAA", "AABAB", "AABBA", "AABBB", "ABAAA", "ABAAB",
        "ABABA", "ABABB", "ABBAA", "ABBAB", "ABBBA", "ABBBB", "BAAAA", "BAAAB", "BAABA", "BAABB",
        "BABAA", "BABAB", "BABBA", "BABBB",
    ];

    message
        .chars()
        .map(|c| {
            if let Some(index) = alphabet.find(c.to_ascii_uppercase()) {
                baconian[index].to_string()
            } else {
                c.to_string()
            }
        })
        .collect()
}


pub fn baconian_decode(encoded: &str) -> String {
    let baconian = [
        "AAAAA", "AAAAB", "AAABA", "AAABB", "AABAA", "AABAB", "AABBA", "AABBB", "ABAAA", "ABAAB",
        "ABABA", "ABABB", "ABBAA", "ABBAB", "ABBBA", "ABBBB", "BAAAA", "BAAAB", "BAABA", "BAABB",
        "BABAA", "BABAB", "BABBA", "BABBB",
    ];
    let alphabet = "ABCDEFGHIJKLMNOPQRSTUVWXYZ";

    encoded
        .as_bytes()
        .chunks(5)
        .map(|chunk| {
            if let Some(index) = baconian
                .iter()
                .position(|&x| x == String::from_utf8_lossy(chunk))
            {
                alphabet.chars().nth(index).unwrap()
            } else {
                ' '
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_baconian_encoding() {
        let message = "HELLO";
        let encoded = baconian_encode(message);
        assert_eq!(encoded, "AABBBAABAAABABBABABBABBBA");
    }

    #[test]
    fn test_baconian_decoding() {
        let message = "AABBBAABAAABABBABABBABBBA";
        let decoded = baconian_decode(message);
        assert_eq!(decoded, "HELLO");
    }
}
