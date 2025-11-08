pub fn manacher(s: String) -> String {
    let l = s.len();
    if l <= 1 {
        return s;
    }

    
    
    
    let mut chars: Vec<char> = Vec::with_capacity(s.len() * 2 + 1);
    for c in s.chars() {
        chars.push('#');
        chars.push(c);
    }
    chars.push('#');

    
    let mut length_of_palindrome = vec![1usize; chars.len()];
    
    let mut current_center: usize = 0;
    
    let mut right_from_current_center: usize = 0;

    for i in 0..chars.len() {
        
        if right_from_current_center > i && i > current_center {
            
            
            length_of_palindrome[i] = std::cmp::min(
                right_from_current_center - i,
                length_of_palindrome[2 * current_center - i],
            );
            
            if length_of_palindrome[i] + i >= right_from_current_center {
                current_center = i;
                right_from_current_center = length_of_palindrome[i] + i;
                
                
                if right_from_current_center >= chars.len() - 1 {
                    break;
                }
            } else {
                
                
                
                continue;
            }
        }

        
        
        
        let mut radius: usize = (length_of_palindrome[i] - 1) / 2;
        radius += 1;
        
        
        while i >= radius && i + radius <= chars.len() - 1 && chars[i - radius] == chars[i + radius]
        {
            length_of_palindrome[i] += 2;
            radius += 1;
        }
    }

    
    let center_of_max = length_of_palindrome
        .iter()
        .enumerate()
        .max_by_key(|&(_, &value)| value)
        .map(|(idx, _)| idx)
        .unwrap();
    let radius_of_max = (length_of_palindrome[center_of_max] - 1) / 2;
    let answer = &chars[(center_of_max - radius_of_max)..(center_of_max + radius_of_max + 1)]
        .iter()
        .collect::<String>();
    answer.replace('#', "")
}

#[cfg(test)]
mod tests {
    use super::manacher;

    #[test]
    fn get_longest_palindrome_by_manacher() {
        assert_eq!(manacher("babad".to_string()), "aba".to_string());
        assert_eq!(manacher("cbbd".to_string()), "bb".to_string());
        assert_eq!(manacher("a".to_string()), "a".to_string());

        let ac_ans = manacher("ac".to_string());
        assert!(ac_ans == *"a" || ac_ans == *"c");
    }
}
