










const MOD: i64 = 1000000007; 
const MAX: usize = 1005; 

pub fn init_catalan() -> Vec<i64> {
    let mut catalan = vec![0; MAX];
    catalan[0] = 1;
    catalan[1] = 1;

    for i in 2..MAX {
        catalan[i] = 0;
        for j in 0..i {
            catalan[i] += (catalan[j] * catalan[i - j - 1]) % MOD;
            if catalan[i] >= MOD {
                catalan[i] -= MOD;
            }
        }
    }

    catalan
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_catalan() {
        let catalan = init_catalan();

        
        assert_eq!(catalan[0], 1);

        
        assert_eq!(catalan[1], 1);

        
        assert_eq!(catalan[5], 42);

        
        assert_eq!(catalan[10], 16796);

        
        assert_eq!(catalan[15], 9694845);

        
        println!("All tests passed!");
    }
}
