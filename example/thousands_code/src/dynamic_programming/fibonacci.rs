
use std::collections::HashMap;






pub fn fibonacci(n: u32) -> u128 {
    
    let mut a = 0;
    let mut b = 1;
    for _i in 0..n {
        
        
        let c = a + b;
        a = b;
        b = c;
    }
    b
}






pub fn recursive_fibonacci(n: u32) -> u128 {
    
    
    _recursive_fibonacci(n, 0, 1)
}

fn _recursive_fibonacci(n: u32, previous: u128, current: u128) -> u128 {
    if n == 0 {
        current
    } else {
        _recursive_fibonacci(n - 1, current, current + previous)
    }
}






pub fn classical_fibonacci(n: u32) -> u128 {
    match n {
        0 => 0,
        1 => 1,
        _ => {
            let k = n / 2;
            let f1 = classical_fibonacci(k);
            let f2 = classical_fibonacci(k - 1);

            match n % 4 {
                0 | 2 => f1 * (f1 + 2 * f2),
                1 => (2 * f1 + f2) * (2 * f1 - f2) + 2,
                _ => (2 * f1 + f2) * (2 * f1 - f2) - 2,
            }
        }
    }
}






pub fn logarithmic_fibonacci(n: u32) -> u128 {
    
    
    if n == 186 {
        let (_, second) = _logarithmic_fibonacci(185);
        second
    } else {
        let (first, _) = _logarithmic_fibonacci(n);
        first
    }
}

fn _logarithmic_fibonacci(n: u32) -> (u128, u128) {
    match n {
        0 => (0, 1),
        _ => {
            let (current, next) = _logarithmic_fibonacci(n / 2);
            let c = current * (next * 2 - current);
            let d = current * current + next * next;

            match n % 2 {
                0 => (c, d),
                _ => (d, c + d),
            }
        }
    }
}


pub fn memoized_fibonacci(n: u32) -> u128 {
    let mut cache: HashMap<u32, u128> = HashMap::new();

    _memoized_fibonacci(n, &mut cache)
}

fn _memoized_fibonacci(n: u32, cache: &mut HashMap<u32, u128>) -> u128 {
    if n == 0 {
        return 0;
    }
    if n == 1 {
        return 1;
    }

    let f = match cache.get(&n) {
        Some(f) => f,
        None => {
            let f1 = _memoized_fibonacci(n - 1, cache);
            let f2 = _memoized_fibonacci(n - 2, cache);
            cache.insert(n, f1 + f2);
            cache.get(&n).unwrap()
        }
    };

    *f
}










pub fn matrix_fibonacci(n: u32) -> u128 {
    let multiplier: Vec<Vec<u128>> = vec![vec![1, 1], vec![1, 0]];

    let multiplier = matrix_power(&multiplier, n);
    let initial_fib_matrix: Vec<Vec<u128>> = vec![vec![1], vec![0]];

    let res = matrix_multiply(&multiplier, &initial_fib_matrix);

    res[1][0]
}

fn matrix_power(base: &Vec<Vec<u128>>, power: u32) -> Vec<Vec<u128>> {
    let identity_matrix: Vec<Vec<u128>> = vec![vec![1, 0], vec![0, 1]];

    vec![base; power as usize]
        .iter()
        .fold(identity_matrix, |acc, x| matrix_multiply(&acc, x))
}


#[allow(clippy::needless_range_loop)]
fn matrix_multiply(multiplier: &[Vec<u128>], multiplicand: &[Vec<u128>]) -> Vec<Vec<u128>> {
    
    
    let mut result: Vec<Vec<u128>> = vec![];
    let mut temp;
    
    let row_right_length = multiplicand[0].len();
    for row_left in 0..multiplier.len() {
        if multiplier[row_left].len() != multiplicand.len() {
            panic!("Matrix dimensions do not match");
        }
        result.push(vec![]);
        for column_right in 0..multiplicand[0].len() {
            temp = 0;
            for row_right in 0..multiplicand.len() {
                if row_right_length != multiplicand[row_right].len() {
                    
                    panic!("Matrix dimensions do not match");
                }
                temp += multiplier[row_left][row_right] * multiplicand[row_right][column_right];
            }
            result[row_left].push(temp);
        }
    }
    result
}










pub fn binary_lifting_fibonacci(n: u32) -> u128 {
    
    let mut state = (0u128, 1u128);

    for i in (0..u32::BITS - n.leading_zeros()).rev() {
        
        state = (
            state.0 * (2 * state.1 - state.0),
            state.0 * state.0 + state.1 * state.1,
        );
        if n & (1 << i) != 0 {
            state = (state.1, state.0 + state.1);
        }
    }

    state.0
}



pub fn nth_fibonacci_number_modulo_m(n: i64, m: i64) -> i128 {
    let (length, pisano_sequence) = get_pisano_sequence_and_period(m);

    let remainder = n % length as i64;
    pisano_sequence[remainder as usize].to_owned()
}




fn get_pisano_sequence_and_period(m: i64) -> (i128, Vec<i128>) {
    let mut a = 0;
    let mut b = 1;
    let mut length: i128 = 0;
    let mut pisano_sequence: Vec<i128> = vec![a, b];

    
    for _i in 0..(m * m) + 1 {
        let c = (a + b) % m as i128;

        
        pisano_sequence.push(c);

        a = b;
        b = c;

        if a == 0 && b == 1 {
            
            
            pisano_sequence.pop();
            pisano_sequence.pop();
            length = pisano_sequence.len() as i128;
            break;
        }
    }

    (length, pisano_sequence)
}







pub fn last_digit_of_the_sum_of_nth_fibonacci_number(n: i64) -> i64 {
    if n < 2 {
        return n;
    }

    
    let n = ((n + 2) % 60) as usize;
    let mut fib = vec![0; n + 1];
    fib[0] = 0;
    fib[1] = 1;

    for i in 2..=n {
        fib[i] = (fib[i - 1] % 10 + fib[i - 2] % 10) % 10;
    }

    if fib[n] == 0 {
        return 9;
    }

    fib[n] % 10 - 1
}

#[cfg(test)]
mod tests {
    use super::binary_lifting_fibonacci;
    use super::classical_fibonacci;
    use super::fibonacci;
    use super::last_digit_of_the_sum_of_nth_fibonacci_number;
    use super::logarithmic_fibonacci;
    use super::matrix_fibonacci;
    use super::memoized_fibonacci;
    use super::nth_fibonacci_number_modulo_m;
    use super::recursive_fibonacci;

    #[test]
    fn test_fibonacci() {
        assert_eq!(fibonacci(0), 1);
        assert_eq!(fibonacci(1), 1);
        assert_eq!(fibonacci(2), 2);
        assert_eq!(fibonacci(3), 3);
        assert_eq!(fibonacci(4), 5);
        assert_eq!(fibonacci(5), 8);
        assert_eq!(fibonacci(10), 89);
        assert_eq!(fibonacci(20), 10946);
        assert_eq!(fibonacci(100), 573147844013817084101);
        assert_eq!(fibonacci(184), 205697230343233228174223751303346572685);
    }

    #[test]
    fn test_recursive_fibonacci() {
        assert_eq!(recursive_fibonacci(0), 1);
        assert_eq!(recursive_fibonacci(1), 1);
        assert_eq!(recursive_fibonacci(2), 2);
        assert_eq!(recursive_fibonacci(3), 3);
        assert_eq!(recursive_fibonacci(4), 5);
        assert_eq!(recursive_fibonacci(5), 8);
        assert_eq!(recursive_fibonacci(10), 89);
        assert_eq!(recursive_fibonacci(20), 10946);
        assert_eq!(recursive_fibonacci(100), 573147844013817084101);
        assert_eq!(
            recursive_fibonacci(184),
            205697230343233228174223751303346572685
        );
    }

    #[test]
    fn test_classical_fibonacci() {
        assert_eq!(classical_fibonacci(0), 0);
        assert_eq!(classical_fibonacci(1), 1);
        assert_eq!(classical_fibonacci(2), 1);
        assert_eq!(classical_fibonacci(3), 2);
        assert_eq!(classical_fibonacci(4), 3);
        assert_eq!(classical_fibonacci(5), 5);
        assert_eq!(classical_fibonacci(10), 55);
        assert_eq!(classical_fibonacci(20), 6765);
        assert_eq!(classical_fibonacci(21), 10946);
        assert_eq!(classical_fibonacci(100), 354224848179261915075);
        assert_eq!(
            classical_fibonacci(184),
            127127879743834334146972278486287885163
        );
    }

    #[test]
    fn test_logarithmic_fibonacci() {
        assert_eq!(logarithmic_fibonacci(0), 0);
        assert_eq!(logarithmic_fibonacci(1), 1);
        assert_eq!(logarithmic_fibonacci(2), 1);
        assert_eq!(logarithmic_fibonacci(3), 2);
        assert_eq!(logarithmic_fibonacci(4), 3);
        assert_eq!(logarithmic_fibonacci(5), 5);
        assert_eq!(logarithmic_fibonacci(10), 55);
        assert_eq!(logarithmic_fibonacci(20), 6765);
        assert_eq!(logarithmic_fibonacci(21), 10946);
        assert_eq!(logarithmic_fibonacci(100), 354224848179261915075);
        assert_eq!(
            logarithmic_fibonacci(184),
            127127879743834334146972278486287885163
        );
    }

    #[test]
    
    
    fn test_iterative_and_recursive_equivalence() {
        assert_eq!(fibonacci(0), recursive_fibonacci(0));
        assert_eq!(fibonacci(1), recursive_fibonacci(1));
        assert_eq!(fibonacci(2), recursive_fibonacci(2));
        assert_eq!(fibonacci(3), recursive_fibonacci(3));
        assert_eq!(fibonacci(4), recursive_fibonacci(4));
        assert_eq!(fibonacci(5), recursive_fibonacci(5));
        assert_eq!(fibonacci(10), recursive_fibonacci(10));
        assert_eq!(fibonacci(20), recursive_fibonacci(20));
        assert_eq!(fibonacci(100), recursive_fibonacci(100));
        assert_eq!(fibonacci(184), recursive_fibonacci(184));
    }

    #[test]
    
    
    
    
    fn test_classical_and_combinatorial_are_off_by_one() {
        assert_eq!(classical_fibonacci(1), fibonacci(0));
        assert_eq!(classical_fibonacci(2), fibonacci(1));
        assert_eq!(classical_fibonacci(3), fibonacci(2));
        assert_eq!(classical_fibonacci(4), fibonacci(3));
        assert_eq!(classical_fibonacci(5), fibonacci(4));
        assert_eq!(classical_fibonacci(6), fibonacci(5));
        assert_eq!(classical_fibonacci(11), fibonacci(10));
        assert_eq!(classical_fibonacci(20), fibonacci(19));
        assert_eq!(classical_fibonacci(21), fibonacci(20));
        assert_eq!(classical_fibonacci(101), fibonacci(100));
        assert_eq!(classical_fibonacci(185), fibonacci(184));
    }

    #[test]
    fn test_memoized_fibonacci() {
        assert_eq!(memoized_fibonacci(0), 0);
        assert_eq!(memoized_fibonacci(1), 1);
        assert_eq!(memoized_fibonacci(2), 1);
        assert_eq!(memoized_fibonacci(3), 2);
        assert_eq!(memoized_fibonacci(4), 3);
        assert_eq!(memoized_fibonacci(5), 5);
        assert_eq!(memoized_fibonacci(10), 55);
        assert_eq!(memoized_fibonacci(20), 6765);
        assert_eq!(memoized_fibonacci(21), 10946);
        assert_eq!(memoized_fibonacci(100), 354224848179261915075);
        assert_eq!(
            memoized_fibonacci(184),
            127127879743834334146972278486287885163
        );
    }

    #[test]
    fn test_matrix_fibonacci() {
        assert_eq!(matrix_fibonacci(0), 0);
        assert_eq!(matrix_fibonacci(1), 1);
        assert_eq!(matrix_fibonacci(2), 1);
        assert_eq!(matrix_fibonacci(3), 2);
        assert_eq!(matrix_fibonacci(4), 3);
        assert_eq!(matrix_fibonacci(5), 5);
        assert_eq!(matrix_fibonacci(10), 55);
        assert_eq!(matrix_fibonacci(20), 6765);
        assert_eq!(matrix_fibonacci(21), 10946);
        assert_eq!(matrix_fibonacci(100), 354224848179261915075);
        assert_eq!(
            matrix_fibonacci(184),
            127127879743834334146972278486287885163
        );
    }

    #[test]
    fn test_binary_lifting_fibonacci() {
        assert_eq!(binary_lifting_fibonacci(0), 0);
        assert_eq!(binary_lifting_fibonacci(1), 1);
        assert_eq!(binary_lifting_fibonacci(2), 1);
        assert_eq!(binary_lifting_fibonacci(3), 2);
        assert_eq!(binary_lifting_fibonacci(4), 3);
        assert_eq!(binary_lifting_fibonacci(5), 5);
        assert_eq!(binary_lifting_fibonacci(10), 55);
        assert_eq!(binary_lifting_fibonacci(20), 6765);
        assert_eq!(binary_lifting_fibonacci(21), 10946);
        assert_eq!(binary_lifting_fibonacci(100), 354224848179261915075);
        assert_eq!(
            binary_lifting_fibonacci(184),
            127127879743834334146972278486287885163
        );
    }

    #[test]
    fn test_nth_fibonacci_number_modulo_m() {
        assert_eq!(nth_fibonacci_number_modulo_m(5, 10), 5);
        assert_eq!(nth_fibonacci_number_modulo_m(10, 7), 6);
        assert_eq!(nth_fibonacci_number_modulo_m(20, 100), 65);
        assert_eq!(nth_fibonacci_number_modulo_m(1, 5), 1);
        assert_eq!(nth_fibonacci_number_modulo_m(0, 15), 0);
        assert_eq!(nth_fibonacci_number_modulo_m(50, 1000), 25);
        assert_eq!(nth_fibonacci_number_modulo_m(100, 37), 7);
        assert_eq!(nth_fibonacci_number_modulo_m(15, 2), 0);
        assert_eq!(nth_fibonacci_number_modulo_m(8, 1_000_000), 21);
        assert_eq!(nth_fibonacci_number_modulo_m(1000, 997), 996);
        assert_eq!(nth_fibonacci_number_modulo_m(200, 123), 0);
    }

    #[test]
    fn test_last_digit_of_the_sum_of_nth_fibonacci_number() {
        assert_eq!(last_digit_of_the_sum_of_nth_fibonacci_number(0), 0);
        assert_eq!(last_digit_of_the_sum_of_nth_fibonacci_number(1), 1);
        assert_eq!(last_digit_of_the_sum_of_nth_fibonacci_number(2), 2);
        assert_eq!(last_digit_of_the_sum_of_nth_fibonacci_number(3), 4);
        assert_eq!(last_digit_of_the_sum_of_nth_fibonacci_number(4), 7);
        assert_eq!(last_digit_of_the_sum_of_nth_fibonacci_number(5), 2);
        assert_eq!(last_digit_of_the_sum_of_nth_fibonacci_number(25), 7);
        assert_eq!(last_digit_of_the_sum_of_nth_fibonacci_number(50), 8);
        assert_eq!(last_digit_of_the_sum_of_nth_fibonacci_number(100), 5);
    }
}
