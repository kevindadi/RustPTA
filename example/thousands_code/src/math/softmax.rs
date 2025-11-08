


















use std::f32::consts::E;

pub fn softmax(array: Vec<f32>) -> Vec<f32> {
    let mut softmax_array = array;

    for value in &mut softmax_array {
        *value = E.powf(*value);
    }

    let sum: f32 = softmax_array.iter().sum();

    for value in &mut softmax_array {
        *value /= sum;
    }

    softmax_array
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_softmax() {
        let test = vec![9.0, 0.5, -3.0, 0.0, 3.0];
        assert_eq!(
            softmax(test),
            vec![
                0.9971961,
                0.00020289792,
                6.126987e-6,
                0.00012306382,
                0.0024718025
            ]
        );
    }
}
