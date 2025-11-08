


pub fn list_subset(
    set: &[i32],
    n: usize,
    r: usize,
    index: usize,
    data: &mut [i32],
    i: usize,
) -> Vec<Vec<i32>> {
    let mut res = Vec::new();

    
    if i == r {
        let mut subset = Vec::new();
        for j in data.iter().take(r) {
            subset.push(*j);
        }
        res.push(subset);
        return res;
    }

    
    if index >= n {
        return res;
    }

    
    data[i] = set[index];
    res.append(&mut list_subset(set, n, r, index + 1, data, i + 1));

    
    
    res.append(&mut list_subset(set, n, r, index + 1, data, i));

    res
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_print_subset3() {
        let set = [1, 2, 3, 4, 5];
        let n = set.len();
        const R: usize = 3;
        let mut data = [0; R];

        let res = list_subset(&set, n, R, 0, &mut data, 0);

        assert_eq!(
            res,
            vec![
                vec![1, 2, 3],
                vec![1, 2, 4],
                vec![1, 2, 5],
                vec![1, 3, 4],
                vec![1, 3, 5],
                vec![1, 4, 5],
                vec![2, 3, 4],
                vec![2, 3, 5],
                vec![2, 4, 5],
                vec![3, 4, 5]
            ]
        );
    }

    #[test]
    fn test_print_subset4() {
        let set = [1, 2, 3, 4, 5];
        let n = set.len();
        const R: usize = 4;
        let mut data = [0; R];

        let res = list_subset(&set, n, R, 0, &mut data, 0);

        assert_eq!(
            res,
            vec![
                vec![1, 2, 3, 4],
                vec![1, 2, 3, 5],
                vec![1, 2, 4, 5],
                vec![1, 3, 4, 5],
                vec![2, 3, 4, 5]
            ]
        );
    }

    #[test]
    fn test_print_subset5() {
        let set = [1, 2, 3, 4, 5];
        let n = set.len();
        const R: usize = 5;
        let mut data = [0; R];

        let res = list_subset(&set, n, R, 0, &mut data, 0);

        assert_eq!(res, vec![vec![1, 2, 3, 4, 5]]);
    }

    #[test]
    fn test_print_incorrect_subset() {
        let set = [1, 2, 3, 4, 5];
        let n = set.len();
        const R: usize = 6;
        let mut data = [0; R];

        let res = list_subset(&set, n, R, 0, &mut data, 0);

        let result_set: Vec<Vec<i32>> = Vec::new();
        assert_eq!(res, result_set);
    }
}
