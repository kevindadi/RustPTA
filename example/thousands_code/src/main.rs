pub mod backtracking;
pub mod big_integer;
pub mod bit_manipulation;
pub mod ciphers;
pub mod compression;
pub mod conversions;
pub mod data_structures;
pub mod dynamic_programming;
pub mod financial;
pub mod general;
pub mod geometry;
pub mod graph;
pub mod greedy;
pub mod machine_learning;
pub mod math;
pub mod navigation;
pub mod number_theory;
pub mod searching;
pub mod sorting;
pub mod string;

use greedy::stable_matching;
use std::collections::HashMap;
use std::sync;
use std::sync::{Arc, Condvar, Mutex};
use std::thread;

use crate::conversions::binary_to_decimal;
use crate::conversions::octal_to_decimal;

use crate::machine_learning::gradient_descent;
use crate::machine_learning::linear_regression;
use crate::number_theory::compute_totient;
use crate::sorting::bubble_sort;
use crate::sorting::quick_sort_3_ways;

fn basic_binary_to_decimal() {
    assert_eq!(binary_to_decimal("0000000110"), Some(6));
    assert_eq!(binary_to_decimal("1000011110"), Some(542));
    assert_eq!(binary_to_decimal("1111111111"), Some(1023));
}

fn test_invalid_octal() {
    let input = "89";
    let expected = Err("Non-octal Value");
    assert_eq!(octal_to_decimal(input), expected);
}

fn test_gradient_descent_optimized() {
    fn derivative_of_square(params: &[f64]) -> Vec<f64> {
        params.iter().map(|x| 2. * x).collect()
    }

    let mut x: Vec<f64> = vec![5.0, 6.0];
    let learning_rate: f64 = 0.03;
    let num_iterations: i32 = 1000;

    let minimized_vector =
        gradient_descent(derivative_of_square, &mut x, learning_rate, num_iterations);

    let test_vector = [0.0, 0.0];

    let tolerance = 1e-6;
    for (minimized_value, test_value) in minimized_vector.iter().zip(test_vector.iter()) {
        assert!((minimized_value - test_value).abs() < tolerance);
    }
}

fn test_linear_regression() {
    assert_eq!(
        linear_regression(vec![(0.0, 0.0), (1.0, 1.0), (2.0, 2.0)]),
        Some((2.220446049250313e-16, 0.9999999999999998))
    );
}

fn test_3() {
    assert_eq!(compute_totient(4), vec![1, 1, 2, 2]);
}

fn test_stable_matching_scenario_1() {
    let men_preferences = HashMap::from([
        (
            "A".to_string(),
            vec!["X".to_string(), "Y".to_string(), "Z".to_string()],
        ),
        (
            "B".to_string(),
            vec!["Y".to_string(), "X".to_string(), "Z".to_string()],
        ),
        (
            "C".to_string(),
            vec!["X".to_string(), "Y".to_string(), "Z".to_string()],
        ),
    ]);

    let women_preferences = HashMap::from([
        (
            "X".to_string(),
            vec!["B".to_string(), "A".to_string(), "C".to_string()],
        ),
        (
            "Y".to_string(),
            vec!["A".to_string(), "B".to_string(), "C".to_string()],
        ),
        (
            "Z".to_string(),
            vec!["A".to_string(), "B".to_string(), "C".to_string()],
        ),
    ]);

    let matches = stable_matching(&men_preferences, &women_preferences);

    let expected_matches1 = HashMap::from([
        ("A".to_string(), "Y".to_string()),
        ("B".to_string(), "X".to_string()),
        ("C".to_string(), "Z".to_string()),
    ]);

    let expected_matches2 = HashMap::from([
        ("A".to_string(), "X".to_string()),
        ("B".to_string(), "Y".to_string()),
        ("C".to_string(), "Z".to_string()),
    ]);

    assert!(matches == expected_matches1 || matches == expected_matches2);
}

struct Foo {
    mu1: sync::Arc<sync::Mutex<i32>>,
    rw1: sync::RwLock<i32>,
    
    
    
    
}

impl Foo {
    fn new() -> Self {
        Self {
            mu1: sync::Arc::new(sync::Mutex::new(1)),
            rw1: sync::RwLock::new(1),
            
            
            
            
        }
    }

    fn sync_mutex_1(&self) {
        let guard1 = self.mu1.lock().unwrap();
        match *guard1 {
            1 => {}
            _ => {
                self.sync_mutex_2();
            }
        };
    }

    fn sync_mutex_2(&self) {
        *self.mu1.lock().unwrap() += 1;
    }

    fn sync_rwlock_read_1(&self) {
        match *self.rw1.read().unwrap() {
            1 => {
                self.sync_rwlock_write_2();
            }
            _ => {
                self.sync_rwlock_read_2();
            }
        };
    }

    fn sync_rwlock_write_1(&self) {
        match *self.rw1.write().unwrap() {
            1 => {
                self.sync_rwlock_write_2();
            }
            _ => {
                self.sync_rwlock_read_2();
            }
        };
    }

    fn sync_rwlock_read_2(&self) {
        let _ = *self.rw1.read().unwrap();
    }

    fn sync_rwlock_write_2(&self) {
        *self.rw1.write().unwrap() += 1;
    }

    
    
    
    
    
    
    
    

    
    
    

    
    
    
    
    
    
    
    
    
    

    
    
    
    
    
    
    
    
    
    

    
    
    

    
    
    

    
    
    
    
    
    
    
    

    
    
    

    
    
    

    
    
    
    
    
    
    
    
    
    

    
    
    
    
    
    
    
    
    
    

    
    
    

    
    
    
}

fn main() {
    let foo1 = Foo::new();
    foo1.sync_mutex_1();
    foo1.sync_mutex_2();
    foo1.sync_rwlock_read_1();
    foo1.sync_rwlock_write_1();
    basic_binary_to_decimal();
    test_gradient_descent_optimized();
    test_linear_regression();

    test_3();
    test_invalid_octal();
    test_stable_matching_scenario_1();
}
