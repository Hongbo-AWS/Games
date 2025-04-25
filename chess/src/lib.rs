use serde::{Deserialize, Serialize};

use std::collections::HashMap;
use std::io;
use std::num::ParseIntError;

pub mod game;
pub mod user;

pub use game::*;
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
        winner: Option<Player>,
    },
    Status {
        board: [[Cell; 11]; 11],
        current_player: Player,
    },
    PlayerDisconnected {
        player: Player,
    },
    PlayerConnected {
        player: Player,
        username: String,
    },
    ServerShutdown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Player {
    X,
    O,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum Cell {
    Empty,
    Marked(Player),
}

#[derive(Debug)]
pub enum GameError {
    InvalidInput(String),
    InvalidPosition(String),
    PositionOccupied(String),
}

impl std::fmt::Display for GameError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            GameError::InvalidInput(msg) => write!(f, "输入错误: {}", msg),
            GameError::InvalidPosition(msg) => write!(f, "位置错误: {}", msg),
            GameError::PositionOccupied(msg) => write!(f, "位置已被占用: {}", msg),
        }
    }
}

pub struct Board {
    pub cells: [[Cell; 11]; 11],
    pub current_player: Player,
}

impl Board {
    pub fn new() -> Self {
        Board {
            cells: [[Cell::Empty; 11]; 11],
            current_player: Player::X,
        }
    }

    pub fn display(&self) {
        println!("\n当前棋盘：");
        for row in self.cells {
            for cell in row {
                match cell {
                    Cell::Empty => print!(" - "),
                    Cell::Marked(Player::X) => print!(" X "),
                    Cell::Marked(Player::O) => print!(" O "),
                }
            }
            println!();
        }
    }

    pub fn make_move(&mut self, row: usize, col: usize) -> Result<(), GameError> {
        if row >= 11 || col >= 11 {
            return Err(GameError::InvalidPosition(format!(
                "行和列必须在 0-10 之间，你输入的是 ({}, {})",
                row, col
            )));
        }
        if self.cells[row][col] != Cell::Empty {
            return Err(GameError::PositionOccupied(format!(
                "位置 ({}, {}) 已经被占用",
                row, col
            )));
        }
        self.cells[row][col] = Cell::Marked(self.current_player);
        self.current_player = match self.current_player {
            Player::X => Player::O,
            Player::O => Player::X,
        };
        Ok(())
    }

    pub fn check_winner(&self) -> Option<Player> {
        // 检查行
        for row in 0..11 {
            for col in 0..7 {
                if self.cells[row][col] == self.cells[row][col + 1]
                    && self.cells[row][col + 1] == self.cells[row][col + 2]
                    && self.cells[row][col + 2] == self.cells[row][col + 3]
                    && self.cells[row][col + 3] == self.cells[row][col + 4]
                {
                    if let Cell::Marked(player) = self.cells[row][col] {
                        return Some(player);
                    }
                }
            }
        }
        // 检查列
        for col in 0..11 {
            for row in 0..7 {
                if self.cells[row][col] == self.cells[row + 1][col]
                    && self.cells[row + 1][col] == self.cells[row + 2][col]
                    && self.cells[row + 2][col] == self.cells[row + 3][col]
                    && self.cells[row + 3][col] == self.cells[row + 4][col]
                {
                    if let Cell::Marked(player) = self.cells[row][col] {
                        return Some(player);
                    }
                }
            }
        }
        // 检查对角线（左上到右下）
        for row in 0..7 {
            for col in 0..7 {
                if self.cells[row][col] == self.cells[row + 1][col + 1]
                    && self.cells[row + 1][col + 1] == self.cells[row + 2][col + 2]
                    && self.cells[row + 2][col + 2] == self.cells[row + 3][col + 3]
                    && self.cells[row + 3][col + 3] == self.cells[row + 4][col + 4]
                {
                    if let Cell::Marked(player) = self.cells[row][col] {
                        return Some(player);
                    }
                }
            }
        }
        // 检查对角线（右上到左下）
        for row in 0..7 {
            for col in 4..11 {
                if self.cells[row][col] == self.cells[row + 1][col - 1]
                    && self.cells[row + 1][col - 1] == self.cells[row + 2][col - 2]
                    && self.cells[row + 2][col - 2] == self.cells[row + 3][col - 3]
                    && self.cells[row + 3][col - 3] == self.cells[row + 4][col - 4]
                {
                    if let Cell::Marked(player) = self.cells[row][col] {
                        return Some(player);
                    }
                }
            }
        }
        None
    }

    pub fn is_full(&self) -> bool {
        self.cells
            .iter()
            .all(|row| row.iter().all(|&cell| cell != Cell::Empty))
    }
}

pub struct Game {
    pub board: Board,
    pub players: HashMap<Player, tokio::sync::mpsc::Sender<GameMessage>>,
}

impl Game {
    pub fn new() -> Self {
        Game {
            board: Board::new(),
            players: HashMap::new(),
        }
    }

    pub async fn add_player(
        &mut self,
        player: Player,
        tx: tokio::sync::mpsc::Sender<GameMessage>,
    ) -> Result<(), GameError> {
        if self.players.len() >= 2 {
            return Err(GameError::InvalidInput("游戏已满".to_string()));
        }
        self.players.insert(player, tx);
        Ok(())
    }

    pub async fn remove_player(&mut self, player: Player) {
        self.players.remove(&player);
        // 通知其他玩家
        for (_, tx) in &self.players {
            tx.send(GameMessage::PlayerDisconnected { player })
                .await
                .unwrap();
        }
        // 如果所有玩家都断开，重置游戏状态
        if self.players.is_empty() {
            self.board = Board::new();
        }
    }

    pub async fn make_move(
        &mut self,
        player: Player,
        row: usize,
        col: usize,
    ) -> Result<(), GameError> {
        if self.board.current_player != player {
            return Err(GameError::InvalidInput("不是你的回合".to_string()));
        }
        self.board.make_move(row, col)?;

        // 通知所有玩家
        for (_, tx) in &self.players {
            tx.send(GameMessage::Move { row, col }).await.unwrap();
        }

        if let Some(winner) = self.board.check_winner() {
            for (_, tx) in &self.players {
                tx.send(GameMessage::GameOver {
                    winner: Some(winner),
                })
                .await
                .unwrap();
            }
        } else if self.board.is_full() {
            for (_, tx) in &self.players {
                tx.send(GameMessage::GameOver { winner: None })
                    .await
                    .unwrap();
            }
        }

        Ok(())
    }
}

fn get_input() -> Result<(usize, usize), GameError> {
    let mut input = String::new();
    io::stdin()
        .read_line(&mut input)
        .map_err(|_| GameError::InvalidInput("无法读取输入".to_string()))?;

    let coords: Result<Vec<usize>, ParseIntError> =
        input.trim().split_whitespace().map(|s| s.parse()).collect();

    let coords = coords.map_err(|_| GameError::InvalidInput("请输入有效的数字".to_string()))?;

    if coords.len() != 2 {
        return Err(GameError::InvalidInput(
            "请输入两个数字（行和列），用空格分隔".to_string(),
        ));
    }

    Ok((coords[0], coords[1]))
}
