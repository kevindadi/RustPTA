


















pub fn huber_loss(actual: &[f64], predicted: &[f64], delta: f64) -> f64 {
    let mut loss: Vec<f64> = Vec::new();
    for (a, p) in actual.iter().zip(predicted.iter()) {
        if (a - p).abs() <= delta {
            loss.push(0.5 * (a - p).powf(2.));
        } else {
            loss.push(delta * (a - p).abs() - (0.5 * delta));
        }
    }

    loss.iter().sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_huber_loss() {
        let test_vector_actual = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        let test_vector = vec![5.0, 7.0, 9.0, 11.0, 13.0];
        assert_eq!(huber_loss(&test_vector_actual, &test_vector, 1.0), 27.5);
    }
}
