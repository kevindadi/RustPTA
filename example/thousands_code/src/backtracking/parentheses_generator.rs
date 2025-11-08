







pub fn generate_parentheses(n: usize) -> Vec<String> {
    let mut result = Vec::new();
    if n > 0 {
        generate("", 0, 0, n, &mut result);
    }
    result
}














fn generate(
    current: &str,
    open_count: usize,
    close_count: usize,
    n: usize,
    result: &mut Vec<String>,
) {
    if current.len() == (n * 2) {
        result.push(current.to_string());
        return;
    }

    if open_count < n {
        let new_str = current.to_string() + "(";
        generate(&new_str, open_count + 1, close_count, n, result);
    }

    if close_count < open_count {
        let new_str = current.to_string() + ")";
        generate(&new_str, open_count, close_count + 1, n, result);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    macro_rules! generate_parentheses_tests {
        ($($name:ident: $test_case:expr,)*) => {
            $(
                #[test]
                fn $name() {
                    let (n, expected_result) = $test_case;
                    assert_eq!(generate_parentheses(n), expected_result);
                }
            )*
        };
    }

    generate_parentheses_tests! {
        test_generate_parentheses_0: (0, Vec::<String>::new()),
        test_generate_parentheses_1: (1, vec!["()"]),
        test_generate_parentheses_2: (2, vec!["(())", "()()"]),
        test_generate_parentheses_3: (3, vec!["((()))", "(()())", "(())()", "()(())", "()()()"]),
        test_generate_parentheses_4: (4, vec!["(((())))", "((()()))", "((())())", "((()))()", "(()(()))", "(()()())", "(()())()", "(())(())", "(())()()", "()((()))", "()(()())", "()(())()", "()()(())", "()()()()"]),
    }
}
