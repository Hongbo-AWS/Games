use serde::{Deserialize, Serialize};

pub mod ai;
pub mod user;

pub use ai::*;
pub use user::*;

#[derive(Debug, Serialize, Deserialize)]
pub enum GameMessage {
    Connect {
        username: String,
    },
    Move {
        row: usize,
        col: usize,
    },
    Error(String),
    GameOver {
        winner: Option<PlayerRole>,
    },
    Status {
        board: [[Option<PlayerRole>; 15]; 15],
        current_player: PlayerRole,
    },
    PlayerDisconnected {
        player: PlayerRole,
    },
    PlayerConnected {
        player: PlayerRole,
        username: String,
    },
    ServerShutdown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PlayerRole {
    Black,
    White,
}

impl PlayerRole {
    pub fn other(&self) -> PlayerRole {
        match self {
            PlayerRole::Black => PlayerRole::White,
            PlayerRole::White => PlayerRole::Black,
        }
    }
}

#[derive(Debug)]
pub enum GameError {
    InvalidInput(String),
    InvalidPosition(String),
    PositionOccupied(String),
    InvalidMove(String),
}

impl std::fmt::Display for GameError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            GameError::InvalidInput(msg) => write!(f, "输入错误: {}", msg),
            GameError::InvalidPosition(msg) => write!(f, "位置错误: {}", msg),
            GameError::PositionOccupied(msg) => write!(f, "位置已被占用: {}", msg),
            GameError::InvalidMove(msg) => write!(f, "移动错误: {}", msg),
        }
    }
}

pub struct Board {
    pub cells: [[Option<PlayerRole>; 15]; 15],
    pub current_player: PlayerRole,
}

impl Board {
    pub fn new() -> Self {
        Board {
            cells: [[None; 15]; 15],
            current_player: PlayerRole::Black,
        }
    }

    pub fn display(&self) {
        println!("\n当前棋盘：");
        for row in self.cells {
            for cell in row {
                match cell {
                    None => print!(" - "),
                    Some(PlayerRole::Black) => print!(" X "),
                    Some(PlayerRole::White) => print!(" O "),
                }
            }
            println!();
        }
    }

    pub fn make_move(&mut self, row: usize, col: usize) -> Result<(), GameError> {
        if row >= 15 || col >= 15 {
            return Err(GameError::InvalidPosition(format!(
                "行和列必须在 0-14 之间，你输入的是 ({}, {})",
                row, col
            )));
        }
        if self.cells[row][col].is_some() {
            return Err(GameError::PositionOccupied(format!(
                "位置 ({}, {}) 已经被占用",
                row, col
            )));
        }
        self.cells[row][col] = Some(self.current_player);
        self.current_player = self.current_player.other();
        Ok(())
    }

    pub fn check_winner(&self) -> Option<PlayerRole> {
        let directions = [
            (0, 1),  // 水平
            (1, 0),  // 垂直
            (1, 1),  // 对角线
            (1, -1), // 反对角线
        ];

        for row in 0..15 {
            for col in 0..15 {
                if let Some(player) = self.cells[row][col] {
                    for &(dr, dc) in &directions {
                        let mut count = 1;
                        let mut r = row as i32;
                        let mut c = col as i32;

                        // 正向检查
                        for _ in 0..4 {
                            r += dr;
                            c += dc;
                            if r < 0 || r >= 15 || c < 0 || c >= 15 {
                                break;
                            }
                            if self.cells[r as usize][c as usize] == Some(player) {
                                count += 1;
                            } else {
                                break;
                            }
                        }

                        // 反向检查
                        r = row as i32;
                        c = col as i32;
                        for _ in 0..4 {
                            r -= dr;
                            c -= dc;
                            if r < 0 || r >= 15 || c < 0 || c >= 15 {
                                break;
                            }
                            if self.cells[r as usize][c as usize] == Some(player) {
                                count += 1;
                            } else {
                                break;
                            }
                        }

                        if count >= 5 {
                            return Some(player);
                        }
                    }
                }
            }
        }
        None
    }

    pub fn is_full(&self) -> bool {
        self.cells
            .iter()
            .all(|row| row.iter().all(|&cell| cell.is_some()))
    }
}
