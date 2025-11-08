























use std::f64::consts::E;

pub fn exponential_linear_unit(vector: &Vec<f64>, alpha: f64) -> Vec<f64> {
    let mut _vector = vector.to_owned();

    for value in &mut _vector {
        if value < &mut 0. {
            *value *= alpha * (E.powf(*value) - 1.);
        }
    }

    _vector
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_exponential_linear_unit() {
        let test_vector = vec![-10., 2., -3., 4., -5., 10., 0.05];
        let alpha = 0.01;
        assert_eq!(
            exponential_linear_unit(&test_vector, alpha),
            vec![
                0.09999546000702375,
                2.0,
                0.028506387948964082,
                4.0,
                0.049663102650045726,
                10.0,
                0.05
            ]
        );
    }
}
