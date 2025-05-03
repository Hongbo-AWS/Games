use std::sync::Arc;

use tokio::{
    sync::{mpsc, Mutex},
    task::JoinHandle,
};

use crate::{Board, Game, GameError, GameMessage, PlayerRole};

pub struct AIPlayer {
    pub player: PlayerRole,
    depth: usize,
    game: Arc<Mutex<Game>>,
}

impl AIPlayer {
    pub fn new(player: PlayerRole, game: Arc<Mutex<Game>>) -> Self {
        Self {
            player,
            depth: 3, // 增加搜索深度
            game,
        }
    }

    pub async fn start(&mut self, mut rx: mpsc::Receiver<GameMessage>) -> JoinHandle<()> {
        tokio::spawn(async move {
            while let Some(message) = rx.recv().await {
                println!("AI 收到消息: {:?}", message);
                match message {
                    GameMessage::Move { row, col } => {
                        println!("AI 收到移动消息: ({}, {})", row, col);
                    }
                    _ => {}
                }
            }
        })
    }

    fn evaluate_position(&self, board: &Board, row: usize, col: usize, player: PlayerRole) -> i32 {
        let mut score = 0;
        let directions = [
            (0, 1),  // 水平
            (1, 0),  // 垂直
            (1, 1),  // 对角线
            (1, -1), // 反对角线
        ];

        // 位置评分：中心位置更有价值
        let center = 7;
        let distance_to_center =
            ((row as i32 - center as i32).abs() + (col as i32 - center as i32).abs()) as i32;
        score += (10 - distance_to_center) * 10;

        // 评估周围棋子
        let mut adjacent_own = 0;
        let mut adjacent_opponent = 0;
        for &(dr, dc) in &directions {
            let r = row as i32 + dr;
            let c = col as i32 + dc;
            if r >= 0 && r < 15 && c >= 0 && c < 15 {
                match board.cells[r as usize][c as usize] {
                    Some(p) if p == player => adjacent_own += 1,
                    Some(_) => adjacent_opponent += 1,
                    None => {}
                }
            }
        }
        score += adjacent_own * 50; // 靠近自己的棋子加分
        score -= adjacent_opponent * 30; // 靠近对手的棋子减分

        for &(dr, dc) in &directions {
            let mut count = 0;
            let mut empty = 0;
            let mut blocked = 0;
            let mut consecutive = true;

            // 正向检查
            for i in 1..5 {
                let r = row as i32 + dr * i;
                let c = col as i32 + dc * i;
                if r < 0 || r >= 15 || c < 0 || c >= 15 {
                    blocked += 1;
                    break;
                }
                match board.cells[r as usize][c as usize] {
                    Some(p) if p == player => {
                        if consecutive {
                            count += 1;
                        }
                    }
                    None => {
                        empty += 1;
                        consecutive = true;
                    }
                    _ => {
                        blocked += 1;
                        consecutive = false;
                    }
                }
            }

            // 反向检查
            consecutive = true;
            for i in 1..5 {
                let r = row as i32 - dr * i;
                let c = col as i32 - dc * i;
                if r < 0 || r >= 15 || c < 0 || c >= 15 {
                    blocked += 1;
                    break;
                }
                match board.cells[r as usize][c as usize] {
                    Some(p) if p == player => {
                        if consecutive {
                            count += 1;
                        }
                    }
                    None => {
                        empty += 1;
                        consecutive = true;
                    }
                    _ => {
                        blocked += 1;
                        consecutive = false;
                    }
                }
            }

            // 计算棋型分数
            if count >= 4 {
                score += 100000; // 必胜
            } else if count == 3 && empty >= 1 {
                score += 10000; // 活四
            } else if count == 2 && empty >= 2 {
                score += 1000; // 活三
            }
        }

        score
    }

    // 模拟下一步
    fn simulate_move(
        &self,
        board: &Board,
        row: usize,
        col: usize,
        player: PlayerRole,
        depth: usize,
    ) -> i32 {
        if depth == 0 {
            return self.evaluate_position(board, row, col, player);
        }

        let mut score = 0;
        let opponent = player.other();

        // 评估当前移动
        score += self.evaluate_position(board, row, col, player);

        // 评估对手可能的回应
        let mut best_opponent_score = 0;
        for r in 0..15 {
            for c in 0..15 {
                if board.cells[r][c].is_none() {
                    let opponent_score = self.simulate_move(board, r, c, opponent, depth - 1);
                    best_opponent_score = best_opponent_score.max(opponent_score);
                }
            }
        }
        score -= best_opponent_score / 2; // 考虑对手的最佳回应

        score
    }

    pub async fn action(&self) -> Result<(), GameError> {
        let mut game = self.game.lock().await;
        let (row, col) = self.make_move(&game.board).unwrap();
        game.make_move(self.player, row, col).await
    }
    pub fn make_move(&self, board: &Board) -> Result<(usize, usize), GameError> {
        let mut best_score = -1;
        let mut best_move = None;
        let opponent = self.player.other();

        // 首先检查是否有必胜的位置
        for row in 0..15 {
            for col in 0..15 {
                if board.cells[row][col].is_none() {
                    let attack_score = self.evaluate_position(board, row, col, self.player);
                    if attack_score >= 100000 {
                        return Ok((row, col));
                    }
                }
            }
        }

        // 检查是否需要防守对手的必胜位置或活四
        for row in 0..15 {
            for col in 0..15 {
                if board.cells[row][col].is_none() {
                    let defense_score = self.evaluate_position(board, row, col, opponent);
                    if defense_score >= 100000 || defense_score >= 10000 {
                        return Ok((row, col));
                    }
                }
            }
        }

        // 寻找最佳进攻位置，考虑对手的回应
        for row in 0..15 {
            for col in 0..15 {
                if board.cells[row][col].is_none() {
                    let total_score = self.simulate_move(board, row, col, self.player, self.depth);
                    if total_score > best_score {
                        best_score = total_score;
                        best_move = Some((row, col));
                    }
                }
            }
        }

        if let Some((row, col)) = best_move {
            Ok((row, col))
        } else {
            Err(GameError::InvalidMove("没有可用的位置".to_string()))
        }
    }
}
