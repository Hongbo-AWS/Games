use chess::{Board, GameError, GameMessage, PlayerRole};
use futures_util::{SinkExt, StreamExt};
use rand::prelude::*;
use rand::rngs::StdRng;
use std::sync::Arc;
use tokio::io::{self, AsyncBufReadExt, BufReader};
use tokio::sync::{broadcast, mpsc, Mutex};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::WebSocketStream;

// 从 lib.rs 导入 handle_game_message
use client::handle_game_message;

pub struct AIPlayer {
    depth: usize,
    rng: StdRng,
}

impl AIPlayer {
    pub fn new() -> Self {
        Self {
            depth: 3, // 增加搜索深度
            rng: StdRng::from_entropy(),
        }
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

    pub fn make_move_simple(
        &mut self,
        board: &Board,
        player: PlayerRole,
    ) -> Result<(usize, usize), GameError> {
        // TODO: 实现 AI 逻辑，选择最佳移动
        // 这里简单实现一个随机移动
        let row = self.rng.gen_range(0..15);
        let col = self.rng.gen_range(0..15);
        Ok((row, col))
    }

    pub fn make_move(
        &self,
        board: &Board,
        player: PlayerRole,
    ) -> Result<(usize, usize), GameError> {
        let mut best_score = -1;
        let mut best_move = None;
        let opponent = player.other();

        // 首先检查是否有必胜的位置
        for row in 0..15 {
            for col in 0..15 {
                if board.cells[row][col].is_none() {
                    let attack_score = self.evaluate_position(board, row, col, player);
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
                    let total_score = self.simulate_move(board, row, col, player, self.depth);
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

#[tokio::main]
async fn main() {
    let url = "ws://localhost:8080";
    println!("正在连接到服务器: {}", url);
    let ai_name = String::from("AI001");

    // 连接到服务器
    let ws_stream = match tokio_tungstenite::connect_async(url).await {
        Ok((ws_stream, _)) => ws_stream,
        Err(e) => {
            eprintln!("连接失败: {}", e);
            return;
        }
    };

    // 运行游戏
    run_game(ws_stream, ai_name).await;
}

async fn run_game(
    ws_stream: WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
    ai_name: String,
) {
    let (mut write, mut read) = ws_stream.split();
    let (tx, mut rx) = mpsc::channel::<Message>(32);
    let board = Arc::new(Mutex::new(Board::new()));
    let (game_over_sender, _) = broadcast::channel::<()>(16);
    let (ai_tx, mut ai_rx) = mpsc::channel::<GameMessage>(32);

    let mut ai_player = AIPlayer::new();

    // 发送连接请求到服务器
    let connect_msg = GameMessage::ConnectRequest {
        username: ai_name.clone(),
    };
    let json = serde_json::to_string(&connect_msg).unwrap();
    if let Err(e) = write.send(Message::Text(json)).await {
        eprintln!("发送连接请求失败: {}", e);
        return;
    }

    println!("AI 玩家 {} 正在连接...", ai_name);

    // 等待连接响应
    let player_role = loop {
        match read.next().await {
            Some(Ok(Message::Text(text))) => {
                if let Ok(GameMessage::ConnectResponse {
                    username,
                    player_role: role,
                }) = serde_json::from_str(&text)
                {
                    println!("收到连接响应: {} 被分配为 {:?}", username, role);
                    break role;
                }
            }
            Some(Err(e)) => {
                eprintln!("接收消息错误: {}", e);
                return;
            }
            None => {
                eprintln!("连接已关闭");
                return;
            }
            _ => continue,
        }
    };

    // 处理 AI 移动的任务
    let ai_task = {
        let board = board.clone();
        let tx = tx.clone();
        let game_over_sender = game_over_sender.clone();
        let mut game_over_receiver = game_over_sender.subscribe();
        tokio::spawn(async move {
            let mut is_my_turn = false;
            loop {
                tokio::select! {
                    _ = game_over_receiver.recv() => {
                        println!("游戏结束标志触发，AI 任务退出");
                        break;
                    }
                    msg = ai_rx.recv() => {
                        if let Some(GameMessage::TurnNotification { player }) = msg {
                            is_my_turn = player == player_role;
                            if is_my_turn {
                                println!("收到回合通知，开始思考移动...");
                                // 等待一段时间，模拟 AI 思考
                                tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;

                                let board = board.lock().await;

                                let (row, col) = ai_player.make_move_simple(&board, player_role).unwrap();

                                let move_msg = GameMessage::Move { row, col };
                                let json = serde_json::to_string(&move_msg).unwrap();
                                println!("AI 发送移动消息: {}", json);
                                if let Err(e) = tx.send(Message::Text(json)).await {
                                    eprintln!("发送消息失败: {}", e);
                                    let _ = game_over_sender.send(());
                                }
                            }
                        }
                    }
                }
            }
            println!("AI 任务结束");
        })
    };

    // 处理接收消息的任务
    let read_task = {
        let board_clone = board.clone();
        let game_over_sender = game_over_sender.clone();
        let mut game_over_receiver = game_over_sender.subscribe();
        let ai_tx = ai_tx.clone();
        tokio::spawn(async move {
            println!("开始监听服务器消息...");
            loop {
                tokio::select! {
                    _ = game_over_receiver.recv() => {
                        println!("游戏结束标志触发，读取任务退出");
                        break;
                    }
                    result = read.next() => {
                        match result {
                            Some(Ok(msg)) => {
                                if let Message::Text(text) = msg {
                                    match serde_json::from_str::<GameMessage>(&text) {
                                        Ok(game_msg) => {
                                            if let GameMessage::TurnNotification { .. } = &game_msg {
                                                let _ = ai_tx.send(game_msg.clone()).await;
                                            }
                                            if handle_game_message(game_msg, &mut *board_clone.lock().await).await {
                                                println!("游戏结束，关闭读取任务");
                                                let _ = game_over_sender.send(());
                                                break;
                                            }
                                        }
                                        Err(e) => eprintln!("解析消息失败: {}", e),
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
            println!("读取任务结束");
        })
    };

    // 处理写入消息的任务
    let write_task = {
        let mut game_over_receiver = game_over_sender.subscribe();
        tokio::spawn(async move {
            let mut write = write;
            loop {
                tokio::select! {
                    maybe_msg = rx.recv() => {
                        match maybe_msg {
                            Some(msg) => {
                                if let Err(e) = write.send(msg).await {
                                    println!("写入任务错误: {}", e);
                                    break;
                                }
                            },
                            None => {
                                println!("通道关闭，写入任务退出");
                                break;
                            }
                        }
                    }
                    _ = game_over_receiver.recv() => {
                        println!("游戏结束标志触发，写入任务退出");
                        break;
                    }
                }
            }
            println!("写入任务结束");
        })
    };

    // 等待所有任务完成
    let (read_result, write_result, ai_result) = tokio::join!(read_task, write_task, ai_task);

    // 检查任务结果
    if let Err(e) = read_result {
        println!("读取任务错误: {}", e);
    }
    if let Err(e) = write_result {
        println!("写入任务错误: {}", e);
    }
    if let Err(e) = ai_result {
        println!("AI 任务错误: {}", e);
    }

    println!("\n游戏已结束，按回车键退出...");
    let mut input = String::new();
    let _ = std::io::stdin().read_line(&mut input);
}
