use std::collections::HashMap;
















pub fn two_sum(nums: Vec<i32>, target: i32) -> Option<(usize, usize)> {
    
    
    
    
    
    
    
    
    
    
    
    
    
    
    
    let mut distance_table: HashMap<i32, usize> = HashMap::new();

    for (i, current_value) in nums.iter().enumerate() {
        match distance_table.get(&(target - current_value)) {
            Some(j) => return Some((i, *j)),
            _ => distance_table.insert(*current_value, i),
        };
    }

    
    None
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test() {
        let nums = vec![2, 7, 11, 15];
        assert_eq!(two_sum(nums, 9), Some((1, 0)));

        let nums = vec![3, 2, 4];
        assert_eq!(two_sum(nums, 6), Some((2, 1)));

        let nums = vec![3, 3];
        assert_eq!(two_sum(nums, 6), Some((1, 0)));

        let nums = vec![2, 7, 11, 15];
        assert_eq!(two_sum(nums, 16), None);
    }
}
