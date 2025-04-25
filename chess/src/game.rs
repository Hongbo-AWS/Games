use serde::{Deserialize, Serialize};
use std::io;
use std::num::ParseIntError;

#[derive(Debug, Serialize, Deserialize)]
pub enum GameError {
    InvalidInput(String),
    InvalidMove(String),
    GameOver,
    IOError(String),
}

impl From<io::Error> for GameError {
    fn from(err: io::Error) -> Self {
        GameError::IOError(err.to_string())
    }
}

impl From<ParseIntError> for GameError {
    fn from(err: ParseIntError) -> Self {
        GameError::InvalidInput(err.to_string())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Player {
    White,
    Black,
}

impl Player {
    pub fn other(&self) -> Player {
        match self {
            Player::White => Player::Black,
            Player::Black => Player::White,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Piece {
    Pawn(Player),
    Knight(Player),
    Bishop(Player),
    Rook(Player),
    Queen(Player),
    King(Player),
}

impl Piece {
    pub fn player(&self) -> Player {
        match self {
            Piece::Pawn(p) => *p,
            Piece::Knight(p) => *p,
            Piece::Bishop(p) => *p,
            Piece::Rook(p) => *p,
            Piece::Queen(p) => *p,
            Piece::King(p) => *p,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Position {
    pub row: usize,
    pub col: usize,
}

impl Position {
    pub fn new(row: usize, col: usize) -> Result<Self, GameError> {
        if row >= 8 || col >= 8 {
            return Err(GameError::InvalidInput(
                "Position out of bounds".to_string(),
            ));
        }
        Ok(Self { row, col })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Move {
    pub from: Position,
    pub to: Position,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Game {
    board: [[Option<Piece>; 8]; 8],
    current_player: Player,
    game_over: bool,
}

impl Game {
    pub fn new() -> Self {
        let mut board = [[None; 8]; 8];

        // 设置白方棋子
        board[0][0] = Some(Piece::Rook(Player::White));
        board[0][1] = Some(Piece::Knight(Player::White));
        board[0][2] = Some(Piece::Bishop(Player::White));
        board[0][3] = Some(Piece::Queen(Player::White));
        board[0][4] = Some(Piece::King(Player::White));
        board[0][5] = Some(Piece::Bishop(Player::White));
        board[0][6] = Some(Piece::Knight(Player::White));
        board[0][7] = Some(Piece::Rook(Player::White));
        for i in 0..8 {
            board[1][i] = Some(Piece::Pawn(Player::White));
        }

        // 设置黑方棋子
        board[7][0] = Some(Piece::Rook(Player::Black));
        board[7][1] = Some(Piece::Knight(Player::Black));
        board[7][2] = Some(Piece::Bishop(Player::Black));
        board[7][3] = Some(Piece::Queen(Player::Black));
        board[7][4] = Some(Piece::King(Player::Black));
        board[7][5] = Some(Piece::Bishop(Player::Black));
        board[7][6] = Some(Piece::Knight(Player::Black));
        board[7][7] = Some(Piece::Rook(Player::Black));
        for i in 0..8 {
            board[6][i] = Some(Piece::Pawn(Player::Black));
        }

        Self {
            board,
            current_player: Player::White,
            game_over: false,
        }
    }

    pub fn make_move(&mut self, mv: Move) -> Result<(), GameError> {
        if self.game_over {
            return Err(GameError::GameOver);
        }

        let from_piece = self.board[mv.from.row][mv.from.col]
            .ok_or_else(|| GameError::InvalidMove("No piece at from position".to_string()))?;

        if from_piece.player() != self.current_player {
            return Err(GameError::InvalidMove("Not your turn".to_string()));
        }

        // TODO: 实现具体的移动规则验证
        self.board[mv.to.row][mv.to.col] = Some(from_piece);
        self.board[mv.from.row][mv.from.col] = None;
        self.current_player = self.current_player.other();

        Ok(())
    }

    pub fn is_game_over(&self) -> bool {
        self.game_over
    }

    pub fn current_player(&self) -> Player {
        self.current_player
    }

    pub fn get_piece(&self, pos: Position) -> Option<Piece> {
        self.board[pos.row][pos.col]
    }
}
