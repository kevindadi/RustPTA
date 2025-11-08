




















pub fn leaky_relu(vector: &Vec<f64>, alpha: f64) -> Vec<f64> {
    let mut _vector = vector.to_owned();

    for value in &mut _vector {
        if value < &mut 0. {
            *value *= alpha;
        }
    }

    _vector
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_leaky_relu() {
        let test_vector = vec![-10., 2., -3., 4., -5., 10., 0.05];
        let alpha = 0.01;
        assert_eq!(
            leaky_relu(&test_vector, alpha),
            vec![-0.1, 2.0, -0.03, 4.0, -0.05, 10.0, 0.05]
        );
    }
}
