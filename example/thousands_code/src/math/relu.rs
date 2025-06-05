









pub fn relu(array: &mut Vec<f32>) -> &mut Vec<f32> {
    
    for value in &mut *array {
        if value <= &mut 0. {
            *value = 0.;
        }
    }

    array
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_relu() {
        let mut test: Vec<f32> = Vec::from([1.0, 0.5, -1.0, 0.0, 0.3]);
        assert_eq!(
            relu(&mut test),
            &mut Vec::<f32>::from([1.0, 0.5, 0.0, 0.0, 0.3])
        );
    }
}
