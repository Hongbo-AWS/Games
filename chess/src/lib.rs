use std::clone;
use std::{collections::HashMap, sync::Arc};

use serde::{Deserialize, Serialize};

pub mod ai;
pub mod user;

pub use ai::*;
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio::sync::Mutex;
pub use user::*;

use futures_util::{SinkExt, StreamExt};

use tokio_tungstenite::accept_async;
use tokio_tungstenite::tungstenite::Message;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum GameMessage {
    ConnectRequest {
        username: String,
    },
    ConnectResponse {
        username: String,
        player_role: PlayerRole,
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
    TurnNotification {
        player: PlayerRole,
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
            (0, 1, "水平"),      // 水平
            (1, 0, "垂直"),      // 垂直
            (1, 1, "对角线"),    // 对角线
            (1, -1, "反对角线"), // 反对角线
        ];

        for row in 0..15 {
            for col in 0..15 {
                if let Some(player) = self.cells[row][col] {
                    for &(dr, dc, direction) in &directions {
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
                            println!(
                                "玩家 {:?} 在 ({}, {}) 位置通过 {} 方向获胜，连续 {} 子",
                                player, row, col, direction, count
                            );
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

pub struct Game {
    board: Board,
    players: HashMap<PlayerRole, mpsc::Sender<GameMessage>>,
}

impl Game {
    pub fn new() -> Self {
        Game {
            board: Board::new(),
            players: HashMap::new(),
        }
    }

    async fn send_turn_notification(&self, player: PlayerRole) {
        if let Some(tx) = self.players.get(&player) {
            let _ = tx.send(GameMessage::TurnNotification { player }).await;
            println!("通知玩家 {:?} 轮到你了", player);
        }
    }

    async fn add_player(
        &mut self,
        player: PlayerRole,
        username: String,
        tx: mpsc::Sender<GameMessage>,
    ) -> Result<(), GameError> {
        if self.players.len() >= 2 {
            return Err(GameError::InvalidInput("游戏已满".to_string()));
        }

        // 发送当前游戏状态给新玩家
        tx.send(GameMessage::Status {
            board: self.board.cells,
            current_player: self.board.current_player,
        })
        .await
        .unwrap();

        self.players.insert(player, tx);

        // 通知其他玩家有新玩家加入
        for (_, other_tx) in &self.players {
            other_tx
                .send(GameMessage::PlayerConnected {
                    player,
                    username: username.clone(),
                })
                .await
                .unwrap();
        }
        println!("通知其他玩家 {} ({:?}) 已加入", username, player);

        // 如果这是第二个玩家，游戏开始，通知当前玩家轮到他了
        if self.players.len() == 2 {
            self.send_turn_notification(self.board.current_player).await;
        }

        Ok(())
    }

    async fn make_move(
        &mut self,
        player: PlayerRole,
        row: usize,
        col: usize,
    ) -> Result<(), GameError> {
        println!("处理移动请求: 玩家 {:?} 移动到 ({}, {})", player, row, col);

        if self.players.len() < 2 {
            println!("移动失败: 等待另一个玩家加入");
            return Err(GameError::InvalidInput("等待另一个玩家加入".to_string()));
        }
        if self.board.current_player != player {
            println!("移动失败: 不是玩家 {:?} 的回合", player);
            return Err(GameError::InvalidInput("不是你的回合".to_string()));
        }

        println!("执行移动: ({}, {})", row, col);
        if let Err(e) = self.board.make_move(row, col) {
            // 移动失败，通知当前玩家继续尝试
            self.send_turn_notification(player).await;
            return Err(e);
        }

        // 通知所有玩家移动和新的游戏状态
        println!("通知所有玩家移动和新的游戏状态");
        for (_, tx) in &self.players {
            tx.send(GameMessage::Move { row, col }).await.unwrap();
            tx.send(GameMessage::Status {
                board: self.board.cells,
                current_player: self.board.current_player,
            })
            .await
            .unwrap();
        }

        // 通知下一个玩家轮到他们了
        self.send_turn_notification(self.board.current_player).await;

        if let Some(winner) = self.board.check_winner() {
            println!("游戏结束！胜利者是: {:?}", winner);
            for (_, tx) in &self.players {
                tx.send(GameMessage::GameOver {
                    winner: Some(winner),
                })
                .await
                .unwrap();
            }
        } else if self.board.is_full() {
            println!("游戏结束！平局！");
            for (_, tx) in &self.players {
                tx.send(GameMessage::GameOver { winner: None })
                    .await
                    .unwrap();
            }
        }

        println!("移动处理完成");
        Ok(())
    }

    async fn remove_player(&mut self, player: PlayerRole) {
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
    pub async fn shutdown(&mut self) {
        println!("服务器正在关闭...");
        // 通知所有玩家服务器关闭
        for (_, tx) in &self.players {
            let _ = tx.send(GameMessage::ServerShutdown).await;
        }
    }

    pub fn get_player_role(&self) -> Option<PlayerRole> {
        if self.players.len() >= 2 {
            println!("游戏已满，拒绝连接");
            return None;
        }
        if self.players.len() == 0 {
            println!("分配玩家角色: Black");
            Some(PlayerRole::Black)
        } else {
            println!("分配玩家角色: White");
            Some(self.players.keys().next().unwrap().other())
        }
    }
    // })
}

pub struct NetworkPlayer {
    stream: TcpStream,
    game: Arc<Mutex<Game>>,
    user_manager: Arc<Mutex<UserManager>>,
}
impl NetworkPlayer {
    pub fn new(
        stream: TcpStream,
        game: Arc<Mutex<Game>>,
        user_manager: Arc<Mutex<UserManager>>,
    ) -> Self {
        Self {
            stream,
            game,
            user_manager,
        }
    }
    pub async fn play(self) {
        let ws_stream = accept_async(self.stream).await.unwrap();
        let (mut ws_sender, mut ws_receiver) = ws_stream.split();

        let (tx, mut rx) = mpsc::channel(32);

        // 等待客户端发送用户名
        let username = match ws_receiver.next().await {
            Some(Ok(Message::Text(text))) => {
                println!("收到连接消息: {}", text);
                match serde_json::from_str::<GameMessage>(&text) {
                    Ok(GameMessage::ConnectRequest { username }) => {
                        println!("新玩家 {} 正在连接...", username);
                        username
                    }
                    Ok(_) => {
                        println!("无效的连接消息类型");
                        let _ = ws_sender
                            .send(Message::Text(
                                serde_json::to_string(&GameMessage::Error(
                                    "无效的连接消息类型".to_string(),
                                ))
                                .unwrap(),
                            ))
                            .await;
                        return;
                    }
                    Err(e) => {
                        println!("解析连接消息失败: {}", e);
                        let _ = ws_sender
                            .send(Message::Text(
                                serde_json::to_string(&GameMessage::Error(
                                    "解析连接消息失败".to_string(),
                                ))
                                .unwrap(),
                            ))
                            .await;
                        return;
                    }
                }
            }
            _ => {
                println!("连接失败：无法读取用户名");
                let _ = ws_sender
                    .send(Message::Text(
                        serde_json::to_string(&GameMessage::Error("连接失败".to_string())).unwrap(),
                    ))
                    .await;
                return;
            }
        };

        // 创建用户
        let user = {
            let mut user_manager = self.user_manager.lock().await;
            let user = user_manager.create_user(username.clone());
            println!("创建用户: {:?}", user);
            user
        };

        // 获取当前游戏状态
        let mut game_guard = self.game.lock().await;
        let player = game_guard.get_player_role();
        if player.is_none() {
            println!("游戏已满，拒绝连接");
            let _ = ws_sender
                .send(Message::Text(
                    serde_json::to_string(&GameMessage::Error("游戏已满".to_string())).unwrap(),
                ))
                .await;
            return;
        }
        let player = player.unwrap();
        // 分配玩家角色给用户
        {
            let mut user_manager = self.user_manager.lock().await;
            if let Err(e) = user_manager.assign_player(&user.id, player) {
                println!("分配玩家角色失败: {}", e);
                let _ = ws_sender
                    .send(Message::Text(
                        serde_json::to_string(&GameMessage::Error(e.to_string())).unwrap(),
                    ))
                    .await;
                return;
            }
            println!("成功分配玩家角色: {:?} 给用户 {}", player, user.name);
        }

        // 添加玩家到游戏
        if let Err(e) = game_guard
            .add_player(player, username.clone(), tx.clone())
            .await
        {
            println!("添加玩家到游戏失败: {}", e);
            let _ = ws_sender
                .send(Message::Text(
                    serde_json::to_string(&GameMessage::Error(e.to_string())).unwrap(),
                ))
                .await;
            return;
        }
        println!("成功添加玩家 {} ({:?}) 到游戏", user.name, player);

        // 发送连接成功消息
        let _ = ws_sender
            .send(Message::Text(
                serde_json::to_string(&GameMessage::ConnectResponse {
                    username: user.name.clone(),
                    player_role: player,
                })
                .unwrap(),
            ))
            .await;
        println!("发送连接成功消息给玩家 {}", user.name);

        drop(game_guard); // 释放锁

        // 处理游戏消息
        let game_clone = self.game.clone();
        let user_manager_clone = self.user_manager.clone();
        let username_clone = username.clone(); // 克隆 username 用于消息处理
        tokio::spawn(async move {
            while let Some(msg) = rx.recv().await {
                println!("发送消息给玩家 {}: {:?}", username_clone, msg);
                let _ = ws_sender
                    .send(Message::Text(serde_json::to_string(&msg).unwrap()))
                    .await;
            }
        });

        // 接收玩家移动
        while let Some(Ok(msg)) = ws_receiver.next().await {
            if let Message::Text(text) = msg {
                println!("收到玩家 {} 的消息: {}", username, text);
                if let Ok(GameMessage::Move { row, col }) = serde_json::from_str(&text) {
                    println!(
                        "玩家 {} ({:?}) 尝试移动: ({}, {})",
                        username, player, row, col
                    );
                    let mut game = game_clone.lock().await;
                    if let Err(e) = game.make_move(player, row, col).await {
                        println!("移动失败: {}", e);
                        tx.send(GameMessage::Error(e.to_string())).await.unwrap();
                    } else {
                        println!("移动成功: ({}, {})", row, col);
                    }
                }
            }
        }

        // 处理断开连接
        {
            println!("玩家 {} ({:?}) 断开连接", user.name, player);
            let mut game = game_clone.lock().await;
            game.remove_player(player).await;
            let mut user_manager = user_manager_clone.lock().await;
            user_manager.remove_user(&user.id);
        }
    }
}
