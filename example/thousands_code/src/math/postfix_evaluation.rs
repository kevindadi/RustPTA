






#[derive(Debug, PartialEq)]
pub enum PostfixError {
    DivisionByZero,
    InvalidOperator,
    InsufficientOperands,
    InvalidExpression,
}



















pub fn evaluate_postfix(expression: &str) -> Result<isize, PostfixError> {
    let mut stack: Vec<isize> = Vec::new();

    for token in expression.split_whitespace() {
        if let Ok(number) = token.parse::<isize>() {
            
            stack.push(number);
        } else {
            
            
            if let (Some(b), Some(a)) = (stack.pop(), stack.pop()) {
                match token {
                    "+" => stack.push(a + b),
                    "-" => stack.push(a - b),
                    "*" => stack.push(a * b),
                    "/" => {
                        if b == 0 {
                            return Err(PostfixError::DivisionByZero);
                        }
                        stack.push(a / b);
                    }
                    _ => return Err(PostfixError::InvalidOperator),
                }
            } else {
                return Err(PostfixError::InsufficientOperands);
            }
        }
    }
    
    if stack.len() == 1 {
        Ok(stack[0])
    } else {
        Err(PostfixError::InvalidExpression)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    macro_rules! postfix_tests {
        ($($name:ident: $test_case:expr,)*) => {
            $(
                #[test]
                fn $name() {
                    let (input, expected) = $test_case;
                    assert_eq!(evaluate_postfix(input), expected);
                }
            )*
        }
    }

    postfix_tests! {
        test_addition_of_two_numbers: ("2 3 +", Ok(5)),
        test_multiplication_and_addition: ("5 2 * 4 +", Ok(14)),
        test_simple_division: ("10 2 /", Ok(5)),
        test_operator_without_operands: ("+", Err(PostfixError::InsufficientOperands)),
        test_division_by_zero_error: ("5 0 /", Err(PostfixError::DivisionByZero)),
        test_invalid_operator_in_expression: ("2 3 #", Err(PostfixError::InvalidOperator)),
        test_missing_operator_for_expression: ("2 3", Err(PostfixError::InvalidExpression)),
        test_extra_operands_in_expression: ("2 3 4 +", Err(PostfixError::InvalidExpression)),
        test_empty_expression_error: ("", Err(PostfixError::InvalidExpression)),
        test_single_number_expression: ("42", Ok(42)),
        test_addition_of_negative_numbers: ("-3 -2 +", Ok(-5)),
        test_complex_expression_with_multiplication_and_addition: ("3 5 8 * 7 + *", Ok(141)),
        test_expression_with_extra_whitespace: ("  3  4   +   ", Ok(7)),
        test_valid_then_invalid_operator: ("5 2 + 1 #", Err(PostfixError::InvalidOperator)),
        test_first_division_by_zero: ("5 0 / 6 0 /", Err(PostfixError::DivisionByZero)),
        test_complex_expression_with_multiple_operators: ("5 1 2 + 4 * + 3 -", Ok(14)),
        test_expression_with_only_whitespace: (" ", Err(PostfixError::InvalidExpression)),
    }
}
