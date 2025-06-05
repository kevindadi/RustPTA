






#[derive(Debug, PartialEq, Eq)]
pub enum MazeError {
    
    EmptyMaze,
    
    OutOfBoundPos,
    
    ImproperMazeRepr,
}


























pub fn find_path_in_maze(
    maze: &[Vec<bool>],
    start_x: usize,
    start_y: usize,
) -> Result<Option<Vec<Vec<bool>>>, MazeError> {
    if maze.is_empty() {
        return Err(MazeError::EmptyMaze);
    }

    
    if start_x >= maze.len() || start_y >= maze[0].len() {
        return Err(MazeError::OutOfBoundPos);
    }

    
    if maze.iter().any(|row| row.len() != maze[0].len()) {
        return Err(MazeError::ImproperMazeRepr);
    }

    
    let maze_instance = Maze::new(maze.to_owned());
    Ok(maze_instance.find_path(start_x, start_y))
}


struct Maze {
    maze: Vec<Vec<bool>>,
}

impl Maze {
    
    const MOVES: [(isize, isize); 4] = [(0, 1), (1, 0), (0, -1), (-1, 0)];

    
    
    
    
    
    
    
    
    
    fn new(maze: Vec<Vec<bool>>) -> Self {
        Maze { maze }
    }

    
    
    
    
    
    fn width(&self) -> usize {
        self.maze[0].len()
    }

    
    
    
    
    
    fn height(&self) -> usize {
        self.maze.len()
    }

    
    
    
    
    
    
    
    
    
    
    fn find_path(&self, start_x: usize, start_y: usize) -> Option<Vec<Vec<bool>>> {
        let mut solution = vec![vec![false; self.width()]; self.height()];
        if self.solve(start_x as isize, start_y as isize, &mut solution) {
            Some(solution)
        } else {
            None
        }
    }

    
    
    
    
    
    
    
    
    
    
    
    fn solve(&self, x: isize, y: isize, solution: &mut [Vec<bool>]) -> bool {
        if x == (self.height() as isize - 1) && y == (self.width() as isize - 1) {
            solution[x as usize][y as usize] = true;
            return true;
        }

        if self.is_valid(x, y, solution) {
            solution[x as usize][y as usize] = true;

            for &(dx, dy) in &Self::MOVES {
                if self.solve(x + dx, y + dy, solution) {
                    return true;
                }
            }

            
            solution[x as usize][y as usize] = false;
            return false;
        }
        false
    }

    
    
    
    
    
    
    
    
    
    
    
    fn is_valid(&self, x: isize, y: isize, solution: &[Vec<bool>]) -> bool {
        x >= 0
            && y >= 0
            && x < self.height() as isize
            && y < self.width() as isize
            && self.maze[x as usize][y as usize]
            && !solution[x as usize][y as usize]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    macro_rules! test_find_path_in_maze {
        ($($name:ident: $start_x:expr, $start_y:expr, $maze:expr, $expected:expr,)*) => {
            $(
                #[test]
                fn $name() {
                    let solution = find_path_in_maze($maze, $start_x, $start_y);
                    assert_eq!(solution, $expected);
                    if let Ok(Some(expected_solution)) = &solution {
                        assert_eq!(expected_solution[$start_x][$start_y], true);
                    }
                }
            )*
        }
    }

    test_find_path_in_maze! {
        maze_with_solution_5x5: 0, 0, &[
            vec![true, false, true, false, false],
            vec![true, true, false, true, false],
            vec![false, true, true, true, false],
            vec![false, false, false, true, true],
            vec![false, true, false, false, true],
        ], Ok(Some(vec![
            vec![true, false, false, false, false],
            vec![true, true, false, false, false],
            vec![false, true, true, true, false],
            vec![false, false, false, true, true],
            vec![false, false, false, false, true],
        ])),
        maze_with_solution_6x6: 0, 0, &[
            vec![true, false, true, false, true, false],
            vec![true, true, false, true, false, true],
            vec![false, true, true, true, true, false],
            vec![false, false, false, true, true, true],
            vec![false, true, false, false, true, false],
            vec![true, true, true, true, true, true],
        ], Ok(Some(vec![
            vec![true, false, false, false, false, false],
            vec![true, true, false, false, false, false],
            vec![false, true, true, true, true, false],
            vec![false, false, false, false, true, false],
            vec![false, false, false, false, true, false],
            vec![false, false, false, false, true, true],
        ])),
        maze_with_solution_8x8: 0, 0, &[
            vec![true, false, false, false, false, false, false, true],
            vec![true, true, false, true, true, true, false, false],
            vec![false, true, true, true, false, false, false, false],
            vec![false, false, false, true, false, true, true, false],
            vec![false, true, false, true, true, true, false, true],
            vec![true, false, true, false, false, true, true, true],
            vec![false, false, true, true, true, false, true, true],
            vec![true, true, true, false, true, true, true, true],
        ], Ok(Some(vec![
            vec![true, false, false, false, false, false, false, false],
            vec![true, true, false, false, false, false, false, false],
            vec![false, true, true, true, false, false, false, false],
            vec![false, false, false, true, false, false, false, false],
            vec![false, false, false, true, true, true, false, false],
            vec![false, false, false, false, false, true, true, true],
            vec![false, false, false, false, false, false, false, true],
            vec![false, false, false, false, false, false, false, true],
        ])),
        maze_without_solution_4x4: 0, 0, &[
            vec![true, false, false, false],
            vec![true, true, false, false],
            vec![false, false, true, false],
            vec![false, false, false, true],
        ], Ok(None::<Vec<Vec<bool>>>),
        maze_with_solution_3x4: 0, 0, &[
            vec![true, false, true, true],
            vec![true, true, true, false],
            vec![false, true, true, true],
        ], Ok(Some(vec![
            vec![true, false, false, false],
            vec![true, true, true, false],
            vec![false, false, true, true],
        ])),
        maze_without_solution_3x4: 0, 0, &[
            vec![true, false, true, true],
            vec![true, false, true, false],
            vec![false, true, false, true],
        ], Ok(None::<Vec<Vec<bool>>>),
        improper_maze_representation: 0, 0, &[
            vec![true],
            vec![true, true],
            vec![true, true, true],
            vec![true, true, true, true]
        ], Err(MazeError::ImproperMazeRepr),
        out_of_bound_start: 0, 3, &[
            vec![true, false, true],
            vec![true, true],
            vec![false, true, true],
        ], Err(MazeError::OutOfBoundPos),
        empty_maze: 0, 0, &[], Err(MazeError::EmptyMaze),
        maze_with_single_cell: 0, 0, &[
            vec![true],
        ], Ok(Some(vec![
                vec![true]
        ])),
        maze_with_one_row_and_multiple_columns: 0, 0, &[
            vec![true, false, true, true, false]
        ], Ok(None::<Vec<Vec<bool>>>),
        maze_with_multiple_rows_and_one_column: 0, 0, &[
            vec![true],
            vec![true],
            vec![false],
            vec![true],
        ], Ok(None::<Vec<Vec<bool>>>),
        maze_with_walls_surrounding_border: 0, 0, &[
            vec![false, false, false],
            vec![false, true, false],
            vec![false, false, false],
        ], Ok(None::<Vec<Vec<bool>>>),
        maze_with_no_walls: 0, 0, &[
            vec![true, true, true],
            vec![true, true, true],
            vec![true, true, true],
        ], Ok(Some(vec![
            vec![true, true, true],
            vec![false, false, true],
            vec![false, false, true],
        ])),
        maze_with_going_back: 0, 0, &[
            vec![true,  true,  true,  true, true,   true],
            vec![false, false, false, true, false,  true],
            vec![true,  true,  true,  true,  false, false],
            vec![true,  false, false, false, false, false],
            vec![true,  false, false, false, true, true],
            vec![true,  false, true,  true,  true,  false],
            vec![true,  false, true , false, true,  false],
            vec![true,  true,  true,  false, true,  true],
        ], Ok(Some(vec![
            vec![true,  true,  true,  true, false,  false],
            vec![false, false, false, true, false,  false],
            vec![true,  true,  true,  true,  false, false],
            vec![true,  false, false, false, false, false],
            vec![true,  false, false, false, false, false],
            vec![true,  false, true,  true,  true,  false],
            vec![true,  false, true , false, true,  false],
            vec![true,  true,  true,  false, true,  true],
        ])),
    }
}
