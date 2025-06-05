







































pub struct Adam {
    learning_rate: f64, 
    betas: (f64, f64),  
    epsilon: f64,       
    m: Vec<f64>,        
    v: Vec<f64>,        
    t: usize,           
}

impl Adam {
    pub fn new(
        learning_rate: Option<f64>,
        betas: Option<(f64, f64)>,
        epsilon: Option<f64>,
        params_len: usize,
    ) -> Self {
        Adam {
            learning_rate: learning_rate.unwrap_or(1e-3), 
            betas: betas.unwrap_or((0.9, 0.999)),         
            epsilon: epsilon.unwrap_or(1e-8),             
            m: vec![0.0; params_len], 
            v: vec![0.0; params_len], 
            t: 0,                     
        }
    }

    pub fn step(&mut self, gradients: &[f64]) -> Vec<f64> {
        let mut model_params = vec![0.0; gradients.len()];
        self.t += 1;

        for i in 0..gradients.len() {
            
            self.m[i] = self.betas.0 * self.m[i] + (1.0 - self.betas.0) * gradients[i];
            self.v[i] = self.betas.1 * self.v[i] + (1.0 - self.betas.1) * gradients[i].powf(2f64);

            
            let m_hat = self.m[i] / (1.0 - self.betas.0.powi(self.t as i32));
            let v_hat = self.v[i] / (1.0 - self.betas.1.powi(self.t as i32));

            
            model_params[i] -= self.learning_rate * m_hat / (v_hat.sqrt() + self.epsilon);
        }
        model_params 
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_adam_init_default_values() {
        let optimizer = Adam::new(None, None, None, 1);

        assert_eq!(optimizer.learning_rate, 0.001);
        assert_eq!(optimizer.betas, (0.9, 0.999));
        assert_eq!(optimizer.epsilon, 1e-8);
        assert_eq!(optimizer.m, vec![0.0; 1]);
        assert_eq!(optimizer.v, vec![0.0; 1]);
        assert_eq!(optimizer.t, 0);
    }

    #[test]
    fn test_adam_init_custom_lr_value() {
        let optimizer = Adam::new(Some(0.9), None, None, 2);

        assert_eq!(optimizer.learning_rate, 0.9);
        assert_eq!(optimizer.betas, (0.9, 0.999));
        assert_eq!(optimizer.epsilon, 1e-8);
        assert_eq!(optimizer.m, vec![0.0; 2]);
        assert_eq!(optimizer.v, vec![0.0; 2]);
        assert_eq!(optimizer.t, 0);
    }

    #[test]
    fn test_adam_init_custom_betas_value() {
        let optimizer = Adam::new(None, Some((0.8, 0.899)), None, 3);

        assert_eq!(optimizer.learning_rate, 0.001);
        assert_eq!(optimizer.betas, (0.8, 0.899));
        assert_eq!(optimizer.epsilon, 1e-8);
        assert_eq!(optimizer.m, vec![0.0; 3]);
        assert_eq!(optimizer.v, vec![0.0; 3]);
        assert_eq!(optimizer.t, 0);
    }

    #[test]
    fn test_adam_init_custom_epsilon_value() {
        let optimizer = Adam::new(None, None, Some(1e-10), 4);

        assert_eq!(optimizer.learning_rate, 0.001);
        assert_eq!(optimizer.betas, (0.9, 0.999));
        assert_eq!(optimizer.epsilon, 1e-10);
        assert_eq!(optimizer.m, vec![0.0; 4]);
        assert_eq!(optimizer.v, vec![0.0; 4]);
        assert_eq!(optimizer.t, 0);
    }

    #[test]
    fn test_adam_init_all_custom_values() {
        let optimizer = Adam::new(Some(1.0), Some((0.001, 0.099)), Some(1e-1), 5);

        assert_eq!(optimizer.learning_rate, 1.0);
        assert_eq!(optimizer.betas, (0.001, 0.099));
        assert_eq!(optimizer.epsilon, 1e-1);
        assert_eq!(optimizer.m, vec![0.0; 5]);
        assert_eq!(optimizer.v, vec![0.0; 5]);
        assert_eq!(optimizer.t, 0);
    }

    #[test]
    fn test_adam_step_default_params() {
        let gradients = vec![-1.0, 2.0, -3.0, 4.0, -5.0, 6.0, -7.0, 8.0];

        let mut optimizer = Adam::new(None, None, None, 8);
        let updated_params = optimizer.step(&gradients);

        assert_eq!(
            updated_params,
            vec![
                0.0009999999900000003,
                -0.000999999995,
                0.0009999999966666666,
                -0.0009999999975,
                0.000999999998,
                -0.0009999999983333334,
                0.0009999999985714286,
                -0.00099999999875
            ]
        );
    }

    #[test]
    fn test_adam_step_custom_params() {
        let gradients = vec![9.0, -8.0, 7.0, -6.0, 5.0, -4.0, 3.0, -2.0, 1.0];

        let mut optimizer = Adam::new(Some(0.005), Some((0.5, 0.599)), Some(1e-5), 9);
        let updated_params = optimizer.step(&gradients);

        assert_eq!(
            updated_params,
            vec![
                -0.004999994444450618,
                0.004999993750007813,
                -0.004999992857153062,
                0.004999991666680556,
                -0.004999990000020001,
                0.004999987500031251,
                -0.004999983333388888,
                0.004999975000124999,
                -0.0049999500004999945
            ]
        );
    }

    #[test]
    fn test_adam_step_empty_gradients_array() {
        let gradients = vec![];

        let mut optimizer = Adam::new(None, None, None, 0);
        let updated_params = optimizer.step(&gradients);

        assert_eq!(updated_params, vec![]);
    }

    #[ignore]
    #[test]
    fn test_adam_step_iteratively_until_convergence_with_default_params() {
        const CONVERGENCE_THRESHOLD: f64 = 1e-5;
        let gradients = vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0];

        let mut optimizer = Adam::new(None, None, None, 6);

        let mut model_params = vec![0.0; 6];
        let mut updated_params = optimizer.step(&gradients);

        while (updated_params
            .iter()
            .zip(model_params.iter())
            .map(|(x, y)| x - y)
            .collect::<Vec<f64>>())
        .iter()
        .map(|&x| x.powi(2))
        .sum::<f64>()
        .sqrt()
            > CONVERGENCE_THRESHOLD
        {
            model_params = updated_params;
            updated_params = optimizer.step(&gradients);
        }

        assert!(updated_params < vec![CONVERGENCE_THRESHOLD; 6]);
        assert_ne!(updated_params, model_params);
        assert_eq!(
            updated_params,
            vec![
                -0.0009999999899999931,
                -0.0009999999949999929,
                -0.0009999999966666597,
                -0.0009999999974999929,
                -0.0009999999979999927,
                -0.0009999999983333263
            ]
        );
    }

    #[ignore]
    #[test]
    fn test_adam_step_iteratively_until_convergence_with_custom_params() {
        const CONVERGENCE_THRESHOLD: f64 = 1e-7;
        let gradients = vec![7.0, -8.0, 9.0, -10.0, 11.0, -12.0, 13.0];

        let mut optimizer = Adam::new(Some(0.005), Some((0.8, 0.899)), Some(1e-5), 7);

        let mut model_params = vec![0.0; 7];
        let mut updated_params = optimizer.step(&gradients);

        while (updated_params
            .iter()
            .zip(model_params.iter())
            .map(|(x, y)| x - y)
            .collect::<Vec<f64>>())
        .iter()
        .map(|&x| x.powi(2))
        .sum::<f64>()
        .sqrt()
            > CONVERGENCE_THRESHOLD
        {
            model_params = updated_params;
            updated_params = optimizer.step(&gradients);
        }

        assert!(updated_params < vec![CONVERGENCE_THRESHOLD; 7]);
        assert_ne!(updated_params, model_params);
        assert_eq!(
            updated_params,
            vec![
                -0.004999992857153061,
                0.004999993750007814,
                -0.0049999944444506185,
                0.004999995000005001,
                -0.004999995454549587,
                0.004999995833336807,
                -0.004999996153849113
            ]
        );
    }
}
