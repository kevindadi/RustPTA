/*
Tic-Tac-Toe Depth First Search Rust Demo
Copyright 2021 David V. Makray

Permission is hereby granted, free of charge, to any person obtaining a copy of
this software and associated documentation files (the "Software"), to deal in
the Software without restriction, including without limitation the rights to
use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies
of the Software, and to permit persons to whom the Software is furnished to do
so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
*/

#[allow(unused_imports)]
use std::io;







#[derive(Copy, Clone, PartialEq, Eq, Debug)]
struct Position {
    x: u8,
    y: u8,
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Players {
    Blank,
    PlayerX,
    PlayerO,
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
struct SinglePlayAction {
    position: Position,
    side: Players,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct PlayActions {
    positions: Vec<Position>,
    side: Players,
}

#[allow(dead_code)]
#[cfg(not(test))]
fn main() {
    let mut board = vec![vec![Players::Blank; 3]; 3];

    while !available_positions(&board).is_empty()
        && !win_check(Players::PlayerX, &board)
        && !win_check(Players::PlayerO, &board)
    {
        display_board(&board);
        println!("Type in coordinate for X mark to be played. ie. a1 etc.");
        let mut input = String::new();
        io::stdin()
            .read_line(&mut input)
            .expect("Failed to read line");

        let mut move_position: Option<Position> = None;
        input.make_ascii_lowercase();
        let bytes = input.trim().trim_start().as_bytes();
        if bytes.len() as u32 == 2
            && (bytes[0] as char).is_alphabetic()
            && (bytes[1] as char).is_numeric()
        {
            let column: u8 = bytes[0] - b'a';
            let row: u8 = bytes[1] - b'1';

            if column <= 2 && row <= 2 {
                move_position = Some(Position { x: column, y: row });
            }
        }

        
        if let Some(move_pos) = move_position {
            let open_positions = available_positions(&board);

            let mut search = open_positions.iter();
            let result = search.find(|&&x| x == move_pos);
            if result.is_none() {
                println!("Not a valid empty coordinate.");
                continue;
            } else {
                board[move_pos.y as usize][move_pos.x as usize] = Players::PlayerX;

                if win_check(Players::PlayerX, &board) {
                    display_board(&board);
                    println!("Player X Wins!");
                    return;
                }
            }

            
            let recusion_result = minimax(Players::PlayerO, &board);
            match recusion_result {
                Some(x) => {
                    
                    
                    
                    let random_selection = 0;

                    let response_pos = x.positions[random_selection];
                    board[response_pos.y as usize][response_pos.x as usize] = Players::PlayerO;
                    if win_check(Players::PlayerO, &board) {
                        display_board(&board);
                        println!("Player O Wins!");
                        return;
                    }
                }

                None => {
                    display_board(&board);
                    println!("Draw game.");
                    return;
                }
            }
        }
    }
}

#[allow(dead_code)]
fn display_board(board: &[Vec<Players>]) {
    println!();
    for (y, board_row) in board.iter().enumerate() {
        print!("{} ", (y + 1));
        for board_cell in board_row {
            match board_cell {
                Players::PlayerX => print!("X "),
                Players::PlayerO => print!("O "),
                Players::Blank => print!("_ "),
            }
        }
        println!();
    }
    println!("  a b c");
}

fn available_positions(board: &[Vec<Players>]) -> Vec<Position> {
    let mut available: Vec<Position> = Vec::new();
    for (y, board_row) in board.iter().enumerate() {
        for (x, board_cell) in board_row.iter().enumerate() {
            if *board_cell == Players::Blank {
                available.push(Position {
                    x: x as u8,
                    y: y as u8,
                });
            }
        }
    }
    available
}

fn win_check(player: Players, board: &[Vec<Players>]) -> bool {
    if player == Players::Blank {
        return false;
    }

    
    if (board[0][0] == board[1][1]) && (board[1][1] == board[2][2]) && (board[2][2] == player)
        || (board[2][0] == board[1][1]) && (board[1][1] == board[0][2]) && (board[0][2] == player)
    {
        return true;
    }

    for i in 0..3 {
        
        if (board[i][0] == board[i][1]) && (board[i][1] == board[i][2]) && (board[i][2] == player) {
            return true;
        }

        
        if (board[0][i] == board[1][i]) && (board[1][i] == board[2][i]) && (board[2][i] == player) {
            return true;
        }
    }

    false
}


pub fn minimax(side: Players, board: &[Vec<Players>]) -> Option<PlayActions> {
    
    if win_check(Players::PlayerX, board) || win_check(Players::PlayerO, board) {
        return None;
    }

    let opposite = match side {
        Players::PlayerX => Players::PlayerO,
        Players::PlayerO => Players::PlayerX,
        Players::Blank => panic!("Minimax can't operate when a player isn't specified."),
    };

    let positions = available_positions(board);
    if positions.is_empty() {
        return None;
    }

    
    let mut best_move: Option<PlayActions> = None;

    for pos in positions {
        let mut board_next = board.to_owned();
        board_next[pos.y as usize][pos.x as usize] = side;

        
        if win_check(Players::PlayerX, &board_next) {
            append_playaction(
                side,
                &mut best_move,
                SinglePlayAction {
                    position: pos,
                    side: Players::PlayerX,
                },
            );
            continue;
        }

        if win_check(Players::PlayerO, &board_next) {
            append_playaction(
                side,
                &mut best_move,
                SinglePlayAction {
                    position: pos,
                    side: Players::PlayerO,
                },
            );
            continue;
        }

        let result = minimax(opposite, &board_next);
        let current_score = match result {
            Some(x) => x.side,
            _ => Players::Blank,
        };

        append_playaction(
            side,
            &mut best_move,
            SinglePlayAction {
                position: pos,
                side: current_score,
            },
        )
    }
    best_move
}


fn append_playaction(
    current_side: Players,
    opt_play_actions: &mut Option<PlayActions>,
    appendee: SinglePlayAction,
) {
    if opt_play_actions.is_none() {
        *opt_play_actions = Some(PlayActions {
            positions: vec![appendee.position],
            side: appendee.side,
        });
        return;
    }

    let play_actions = opt_play_actions.as_mut().unwrap();

    
    match (current_side, play_actions.side, appendee.side) {
        (Players::Blank, _, _) => panic!("Unreachable state."),

        
        (Players::PlayerX, Players::PlayerX, Players::PlayerX) => {
            play_actions.positions.push(appendee.position);
        }
        (Players::PlayerX, Players::PlayerX, _) => {}
        (Players::PlayerO, Players::PlayerO, Players::PlayerO) => {
            play_actions.positions.push(appendee.position);
        }
        (Players::PlayerO, Players::PlayerO, _) => {}

        
        (Players::PlayerX, _, Players::PlayerX) => {
            play_actions.side = Players::PlayerX;
            play_actions.positions.clear();
            play_actions.positions.push(appendee.position);
        }
        (Players::PlayerO, _, Players::PlayerO) => {
            play_actions.side = Players::PlayerO;
            play_actions.positions.clear();
            play_actions.positions.push(appendee.position);
        }

        
        (Players::PlayerX, Players::PlayerO, Players::Blank) => {
            play_actions.side = Players::Blank;
            play_actions.positions.clear();
            play_actions.positions.push(appendee.position);
        }

        (Players::PlayerO, Players::PlayerX, Players::Blank) => {
            play_actions.side = Players::Blank;
            play_actions.positions.clear();
            play_actions.positions.push(appendee.position);
        }

        
        (Players::PlayerX, Players::Blank, Players::PlayerO) => {}
        (Players::PlayerO, Players::Blank, Players::PlayerX) => {}

        
        (_, _, _) => {
            assert!(play_actions.side == appendee.side);
            play_actions.positions.push(appendee.position);
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn win_state_check() {
        let mut board = vec![vec![Players::Blank; 3]; 3];
        board[0][0] = Players::PlayerX;
        board[0][1] = Players::PlayerX;
        board[0][2] = Players::PlayerX;
        let responses = minimax(Players::PlayerO, &board);
        assert_eq!(responses, None);
    }

    #[test]
    fn win_state_check2() {
        let mut board = vec![vec![Players::Blank; 3]; 3];
        board[0][0] = Players::PlayerX;
        board[0][1] = Players::PlayerO;
        board[1][0] = Players::PlayerX;
        board[1][1] = Players::PlayerO;
        board[2][1] = Players::PlayerO;
        let responses = minimax(Players::PlayerO, &board);
        assert_eq!(responses, None);
    }

    #[test]
    fn block_win_move() {
        let mut board = vec![vec![Players::Blank; 3]; 3];
        board[0][0] = Players::PlayerX;
        board[0][1] = Players::PlayerX;
        board[1][2] = Players::PlayerO;
        board[2][2] = Players::PlayerO;
        let responses = minimax(Players::PlayerX, &board);
        assert_eq!(
            responses,
            Some(PlayActions {
                positions: vec![Position { x: 2, y: 0 }],
                side: Players::PlayerX
            })
        );
    }

    #[test]
    fn block_move() {
        let mut board = vec![vec![Players::Blank; 3]; 3];
        board[0][1] = Players::PlayerX;
        board[0][2] = Players::PlayerO;
        board[2][0] = Players::PlayerO;
        let responses = minimax(Players::PlayerX, &board);
        assert_eq!(
            responses,
            Some(PlayActions {
                positions: vec![Position { x: 1, y: 1 }],
                side: Players::Blank
            })
        );
    }

    #[test]
    fn expected_loss() {
        let mut board = vec![vec![Players::Blank; 3]; 3];
        board[0][0] = Players::PlayerX;
        board[0][2] = Players::PlayerO;
        board[1][0] = Players::PlayerX;
        board[2][0] = Players::PlayerO;
        board[2][2] = Players::PlayerO;
        let responses = minimax(Players::PlayerX, &board);
        assert_eq!(
            responses,
            Some(PlayActions {
                positions: vec![
                    Position { x: 1, y: 0 },
                    Position { x: 1, y: 1 },
                    Position { x: 2, y: 1 },
                    Position { x: 1, y: 2 }
                ],
                side: Players::PlayerO
            })
        );
    }
}
