











pub fn cross_product(vec1: [f64; 3], vec2: [f64; 3]) -> [f64; 3] {
    let x = vec1[1] * vec2[2] - vec1[2] * vec2[1];
    let y = -(vec1[0] * vec2[2] - vec1[2] * vec2[0]);
    let z = vec1[0] * vec2[1] - vec1[1] * vec2[0];
    [x, y, z]
}


pub fn vector_magnitude(vec: [f64; 3]) -> f64 {
    (vec[0].powi(2) + vec[1].powi(2) + vec[2].powi(2)).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cross_product_and_magnitude_1() {
        
        let vec1 = [1.0, 2.0, 3.0];
        let vec2 = [4.0, 5.0, 6.0];

        let cross_product = cross_product(vec1, vec2);
        let magnitude = vector_magnitude(cross_product);

        
        assert_eq!(cross_product, [-3.0, 6.0, -3.0]);
        assert!((magnitude - 7.34847).abs() < 1e-5);
    }

    #[test]
    fn test_cross_product_and_magnitude_2() {
        
        let vec1 = [1.0, 0.0, 0.0];
        let vec2 = [0.0, 1.0, 0.0];

        let cross_product = cross_product(vec1, vec2);
        let magnitude = vector_magnitude(cross_product);

        
        assert_eq!(cross_product, [0.0, 0.0, 1.0]);
        assert_eq!(magnitude, 1.0);
    }

    #[test]
    fn test_cross_product_and_magnitude_3() {
        
        let vec1 = [2.0, 0.0, 0.0];
        let vec2 = [0.0, 3.0, 0.0];

        let cross_product = cross_product(vec1, vec2);
        let magnitude = vector_magnitude(cross_product);

        
        assert_eq!(cross_product, [0.0, 0.0, 6.0]);
        assert_eq!(magnitude, 6.0);
    }

    #[test]
    fn test_cross_product_and_magnitude_4() {
        
        let vec1 = [1.0, 2.0, 3.0];
        let vec2 = [2.0, 4.0, 6.0];

        let cross_product = cross_product(vec1, vec2);
        let magnitude = vector_magnitude(cross_product);

        
        assert_eq!(cross_product, [0.0, 0.0, 0.0]);
        assert_eq!(magnitude, 0.0);
    }

    #[test]
    fn test_cross_product_and_magnitude_5() {
        
        let vec1 = [0.0, 0.0, 0.0];
        let vec2 = [0.0, 0.0, 0.0];

        let cross_product = cross_product(vec1, vec2);
        let magnitude = vector_magnitude(cross_product);

        
        assert_eq!(cross_product, [0.0, 0.0, 0.0]);
        assert_eq!(magnitude, 0.0);
    }
}
