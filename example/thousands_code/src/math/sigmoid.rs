









use std::f32::consts::E;

pub fn sigmoid(array: &mut Vec<f32>) -> &mut Vec<f32> {
    
    for value in &mut *array {
        *value = 1. / (1. + E.powf(-1. * *value));
    }

    array
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sigmoid() {
        let mut test = Vec::from([1.0, 0.5, -1.0, 0.0, 0.3]);
        assert_eq!(
            sigmoid(&mut test),
            &mut Vec::<f32>::from([0.7310586, 0.62245935, 0.26894143, 0.5, 0.5744425,])
        );
    }
}
