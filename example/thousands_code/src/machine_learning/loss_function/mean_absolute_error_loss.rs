














pub fn mae_loss(predicted: &[f64], actual: &[f64]) -> f64 {
    let mut total_loss: f64 = 0.0;
    for (p, a) in predicted.iter().zip(actual.iter()) {
        let diff: f64 = p - a;
        let absolute_diff = diff.abs();
        total_loss += absolute_diff;
    }
    total_loss / (predicted.len() as f64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mae_loss() {
        let predicted_values: Vec<f64> = vec![1.0, 2.0, 3.0, 4.0];
        let actual_values: Vec<f64> = vec![1.0, 3.0, 3.5, 4.5];
        assert_eq!(mae_loss(&predicted_values, &actual_values), 0.5);
    }
}
