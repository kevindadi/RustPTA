











pub fn sudoku_solver(board: &[[u8; 9]; 9]) -> Option<[[u8; 9]; 9]> {
    let mut solver = SudokuSolver::new(*board);
    if solver.solve() {
        Some(solver.board)
    } else {
        None
    }
}


struct SudokuSolver {
    
    board: [[u8; 9]; 9],
}

impl SudokuSolver {
    
    fn new(board: [[u8; 9]; 9]) -> SudokuSolver {
        SudokuSolver { board }
    }

    
    
    
    fn find_empty_cell(&self) -> Option<(usize, usize)> {
        
        for row in 0..9 {
            for column in 0..9 {
                if self.board[row][column] == 0 {
                    return Some((row, column));
                }
            }
        }

        None
    }

    
    
    
    fn is_value_valid(&self, coordinates: (usize, usize), value: u8) -> bool {
        let (row, column) = coordinates;

        
        
        for current_column in 0..9 {
            if self.board[row][current_column] == value {
                return false;
            }
        }

        
        for current_row in 0..9 {
            if self.board[current_row][column] == value {
                return false;
            }
        }

        
        let start_row = row / 3 * 3;
        let start_column = column / 3 * 3;

        for current_row in start_row..start_row + 3 {
            for current_column in start_column..start_column + 3 {
                if self.board[current_row][current_column] == value {
                    return false;
                }
            }
        }

        true
    }

    
    
    
    fn solve(&mut self) -> bool {
        let empty_cell = self.find_empty_cell();

        if let Some((row, column)) = empty_cell {
            for value in 1..=9 {
                if self.is_value_valid((row, column), value) {
                    self.board[row][column] = value;
                    if self.solve() {
                        return true;
                    }
                    
                    self.board[row][column] = 0;
                }
            }
        } else {
            
            return true;
        }

        
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    macro_rules! test_sudoku_solver {
        ($($name:ident: $board:expr, $expected:expr,)*) => {
            $(
                #[test]
                fn $name() {
                    let result = sudoku_solver(&$board);
                    assert_eq!(result, $expected);
                }
            )*
        };
    }

    test_sudoku_solver! {
        test_sudoku_correct: [
            [3, 0, 6, 5, 0, 8, 4, 0, 0],
            [5, 2, 0, 0, 0, 0, 0, 0, 0],
            [0, 8, 7, 0, 0, 0, 0, 3, 1],
            [0, 0, 3, 0, 1, 0, 0, 8, 0],
            [9, 0, 0, 8, 6, 3, 0, 0, 5],
            [0, 5, 0, 0, 9, 0, 6, 0, 0],
            [1, 3, 0, 0, 0, 0, 2, 5, 0],
            [0, 0, 0, 0, 0, 0, 0, 7, 4],
            [0, 0, 5, 2, 0, 6, 3, 0, 0],
        ], Some([
            [3, 1, 6, 5, 7, 8, 4, 9, 2],
            [5, 2, 9, 1, 3, 4, 7, 6, 8],
            [4, 8, 7, 6, 2, 9, 5, 3, 1],
            [2, 6, 3, 4, 1, 5, 9, 8, 7],
            [9, 7, 4, 8, 6, 3, 1, 2, 5],
            [8, 5, 1, 7, 9, 2, 6, 4, 3],
            [1, 3, 8, 9, 4, 7, 2, 5, 6],
            [6, 9, 2, 3, 5, 1, 8, 7, 4],
            [7, 4, 5, 2, 8, 6, 3, 1, 9],
        ]),

        test_sudoku_incorrect: [
            [6, 0, 3, 5, 0, 8, 4, 0, 0],
            [5, 2, 0, 0, 0, 0, 0, 0, 0],
            [0, 8, 7, 0, 0, 0, 0, 3, 1],
            [0, 0, 3, 0, 1, 0, 0, 8, 0],
            [9, 0, 0, 8, 6, 3, 0, 0, 5],
            [0, 5, 0, 0, 9, 0, 6, 0, 0],
            [1, 3, 0, 0, 0, 0, 2, 5, 0],
            [0, 0, 0, 0, 0, 0, 0, 7, 4],
            [0, 0, 5, 2, 0, 6, 3, 0, 0],
        ], None::<[[u8; 9]; 9]>,
    }
}
