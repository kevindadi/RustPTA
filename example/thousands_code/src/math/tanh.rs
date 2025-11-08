









use std::f32::consts::E;

pub fn tanh(array: &mut Vec<f32>) -> &mut Vec<f32> {
    
    for value in &mut *array {
        *value = (2. / (1. + E.powf(-2. * *value))) - 1.;
    }

    array
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tanh() {
        let mut test = Vec::from([1.0, 0.5, -1.0, 0.0, 0.3]);
        assert_eq!(
            tanh(&mut test),
            &mut Vec::<f32>::from([0.76159406, 0.4621172, -0.7615941, 0.0, 0.29131258,])
        );
    }
}
