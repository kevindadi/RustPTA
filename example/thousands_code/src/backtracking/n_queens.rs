




















pub fn n_queens_solver(n: usize) -> Vec<Vec<String>> {
    let mut solver = NQueensSolver::new(n);
    solver.solve()
}


struct NQueensSolver {
    
    size: usize,
    
    board: Vec<Vec<char>>,
    
    solutions: Vec<Vec<String>>,
}

impl NQueensSolver {
    
    
    
    
    
    
    
    
    
    fn new(size: usize) -> Self {
        NQueensSolver {
            size,
            board: vec![vec!['.'; size]; size],
            solutions: Vec::new(),
        }
    }

    
    
    
    
    
    fn solve(&mut self) -> Vec<Vec<String>> {
        self.solve_helper(0);
        std::mem::take(&mut self.solutions)
    }

    
    
    
    
    
    
    
    
    
    
    fn is_safe(&self, row: usize, col: usize) -> bool {
        
        for i in 0..row {
            if self.board[i][col] == 'Q'
                || (col >= row - i && self.board[i][col - (row - i)] == 'Q')
                || (col + row - i < self.size && self.board[i][col + (row - i)] == 'Q')
            {
                return false;
            }
        }
        true
    }

    
    
    
    
    
    fn solve_helper(&mut self, row: usize) {
        if row == self.size {
            self.solutions
                .push(self.board.iter().map(|row| row.iter().collect()).collect());
            return;
        }

        for col in 0..self.size {
            if self.is_safe(row, col) {
                self.board[row][col] = 'Q';
                self.solve_helper(row + 1);
                self.board[row][col] = '.';
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    macro_rules! test_n_queens_solver {
        ($($name:ident: $tc:expr,)*) => {
            $(
                #[test]
                fn $name() {
                    let (n, expected_solutions) = $tc;
                    let solutions = n_queens_solver(n);
                    assert_eq!(solutions, expected_solutions);
                }
            )*
        };
    }

    test_n_queens_solver! {
        test_0_queens: (0, vec![Vec::<String>::new()]),
        test_1_queen: (1, vec![vec!["Q"]]),
        test_2_queens:(2, Vec::<Vec<String>>::new()),
        test_3_queens:(3, Vec::<Vec<String>>::new()),
        test_4_queens: (4, vec![
            vec![".Q..",
                 "...Q",
                 "Q...",
                 "..Q."],
            vec!["..Q.",
                 "Q...",
                 "...Q",
                 ".Q.."],
        ]),
        test_5_queens:(5, vec![
            vec!["Q....",
                 "..Q..",
                 "....Q",
                 ".Q...",
                 "...Q."],
            vec!["Q....",
                 "...Q.",
                 ".Q...",
                 "....Q",
                 "..Q.."],
            vec![".Q...",
                 "...Q.",
                 "Q....",
                 "..Q..",
                 "....Q"],
            vec![".Q...",
                 "....Q",
                 "..Q..",
                 "Q....",
                 "...Q."],
            vec!["..Q..",
                 "Q....",
                 "...Q.",
                 ".Q...",
                 "....Q"],
            vec!["..Q..",
                 "....Q",
                 ".Q...",
                 "...Q.",
                 "Q...."],
            vec!["...Q.",
                 "Q....",
                 "..Q..",
                 "....Q",
                 ".Q..."],
            vec!["...Q.",
                 ".Q...",
                 "....Q",
                 "..Q..",
                 "Q...."],
            vec!["....Q",
                 ".Q...",
                 "...Q.",
                 "Q....",
                 "..Q.."],
            vec!["....Q",
                 "..Q..",
                 "Q....",
                 "...Q.",
                 ".Q..."],
        ]),
        test_6_queens: (6, vec![
            vec![".Q....",
                 "...Q..",
                 ".....Q",
                 "Q.....",
                 "..Q...",
                 "....Q."],
            vec!["..Q...",
                 ".....Q",
                 ".Q....",
                 "....Q.",
                 "Q.....",
                 "...Q.."],
            vec!["...Q..",
                 "Q.....",
                 "....Q.",
                 ".Q....",
                 ".....Q",
                 "..Q..."],
            vec!["....Q.",
                 "..Q...",
                 "Q.....",
                 ".....Q",
                 "...Q..",
                 ".Q...."],
        ]),
    }
}
