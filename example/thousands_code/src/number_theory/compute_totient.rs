







use std::vec;

pub fn compute_totient(n: i32) -> vec::Vec<i32> {
    let mut phi: Vec<i32> = Vec::new();

    
    for i in 0..=n {
        phi.push(i);
    }

    
    for p in 2..n + 1 {
        
        
        if phi[(p) as usize] == p {
            
            
            phi[(p) as usize] = p - 1;

            
            
            for i in ((2 * p)..n + 1).step_by(p as usize) {
                phi[(i) as usize] = (phi[i as usize] / p) * (p - 1);
            }
        }
    }

    phi[1..].to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_1() {
        assert_eq!(
            compute_totient(12),
            vec![1, 1, 2, 2, 4, 2, 6, 4, 6, 4, 10, 4]
        );
    }

    #[test]
    fn test_2() {
        assert_eq!(compute_totient(7), vec![1, 1, 2, 2, 4, 2, 6]);
    }

    #[test]
    fn test_3() {
        assert_eq!(compute_totient(4), vec![1, 1, 2, 2]);
    }
}
