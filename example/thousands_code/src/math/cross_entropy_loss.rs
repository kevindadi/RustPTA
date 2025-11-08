

















pub fn cross_entropy_loss(actual: &[f64], predicted: &[f64]) -> f64 {
    let mut loss: Vec<f64> = Vec::new();
    for (a, p) in actual.iter().zip(predicted.iter()) {
        loss.push(-a * p.ln());
    }
    loss.iter().sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cross_entropy_loss() {
        let test_vector_actual = vec![0., 1., 0., 0., 0., 0.];
        let test_vector = vec![0.1, 0.7, 0.1, 0.05, 0.05, 0.1];
        assert_eq!(
            cross_entropy_loss(&test_vector_actual, &test_vector),
            0.35667494393873245
        );
    }
}
