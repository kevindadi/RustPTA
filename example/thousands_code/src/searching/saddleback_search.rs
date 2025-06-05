



use std::cmp::Ordering;

pub fn saddleback_search(matrix: &[Vec<i32>], element: i32) -> (usize, usize) {
    
    let mut left_index = 0;
    let mut right_index = matrix[0].len() - 1;

    
    while left_index < matrix.len() {
        match element.cmp(&matrix[left_index][right_index]) {
            
            Ordering::Equal => return (left_index + 1, right_index + 1),
            Ordering::Greater => {
                
                left_index += 1;
            }
            Ordering::Less => {
                
                if right_index == 0 {
                    break; 
                } else {
                    right_index -= 1;
                }
            }
        }
    }

    
    (0, 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    
    #[test]
    fn test_element_not_found() {
        let matrix = vec![vec![1, 10, 100], vec![2, 20, 200], vec![3, 30, 300]];
        assert_eq!(saddleback_search(&matrix, 123), (0, 0));
    }

    
    #[test]
    fn test_element_at_top_left() {
        let matrix = vec![vec![1, 10, 100], vec![2, 20, 200], vec![3, 30, 300]];
        assert_eq!(saddleback_search(&matrix, 1), (1, 1));
    }

    
    #[test]
    fn test_element_at_bottom_right() {
        let matrix = vec![vec![1, 10, 100], vec![2, 20, 200], vec![3, 30, 300]];
        assert_eq!(saddleback_search(&matrix, 300), (3, 3));
    }

    
    #[test]
    fn test_element_at_top_right() {
        let matrix = vec![vec![1, 10, 100], vec![2, 20, 200], vec![3, 30, 300]];
        assert_eq!(saddleback_search(&matrix, 100), (1, 3));
    }

    
    #[test]
    fn test_element_at_bottom_left() {
        let matrix = vec![vec![1, 10, 100], vec![2, 20, 200], vec![3, 30, 300]];
        assert_eq!(saddleback_search(&matrix, 3), (3, 1));
    }

    
    #[test]
    fn test_element_in_middle() {
        let matrix = vec![
            vec![1, 10, 100, 1000],
            vec![2, 20, 200, 2000],
            vec![3, 30, 300, 3000],
        ];
        assert_eq!(saddleback_search(&matrix, 200), (2, 3));
    }
}
