use chess::{AIPlayer, Board, GameError, GameMessage, PlayerRole, UserManager};
use futures_util::{SinkExt, StreamExt};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::net::{TcpListener, TcpStream};
use tokio::signal;
use tokio::sync::{mpsc, Mutex};
use tokio_tungstenite::accept_async;
use tokio_tungstenite::tungstenite::Message;

pub struct Game {
    board: Board,
    players: HashMap<PlayerRole, mpsc::Sender<GameMessage>>,
    ai_player: Option<AIPlayer>,
}

impl Game {
    fn new() -> Self {
        Game {
            board: Board::new(),
            players: HashMap::new(),
            ai_player: None,
        }
    }

    async fn add_player(
        &mut self,
        player: PlayerRole,
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
        Ok(())
    }

    async fn make_move(
        &mut self,
        player: PlayerRole,
        row: usize,
        col: usize,
    ) -> Result<(), GameError> {
        println!("处理移动请求: 玩家 {:?} 移动到 ({}, {})", player, row, col);

        if self.players.len() < 2 && self.ai_player.is_none() {
            println!("移动失败: 等待另一个玩家加入");
            return Err(GameError::InvalidInput("等待另一个玩家加入".to_string()));
        }
        if self.board.current_player != player {
            println!("移动失败: 不是玩家 {:?} 的回合", player);
            return Err(GameError::InvalidInput("不是你的回合".to_string()));
        }

        println!("执行移动: ({}, {})", row, col);
        self.board.make_move(row, col)?;

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
        } else if let Some(ai) = &self.ai_player {
            println!("AI 玩家回合");
            if let Ok((row, col)) = ai.make_move(&mut self.board) {
                println!("AI 移动到: ({}, {})", row, col);
                self.board.make_move(row, col)?;
                for (_, tx) in &self.players {
                    tx.send(GameMessage::Move { row, col }).await.unwrap();
                    tx.send(GameMessage::Status {
                        board: self.board.cells,
                        current_player: self.board.current_player,
                    })
                    .await
                    .unwrap();
                }

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
            self.ai_player = None;
        }
    }
    async fn shutdown(&mut self) {
        println!("服务器正在关闭...");
        // 通知所有玩家服务器关闭
        for (_, tx) in &self.players {
            let _ = tx.send(GameMessage::ServerShutdown).await;
        }
    }

    fn set_ai_player(&mut self, player: PlayerRole) {
        self.ai_player = Some(AIPlayer::new(player));
    }

    fn remove_ai_player(&mut self) {
        self.ai_player = None;
    }
}

#[tokio::main]
async fn main() {
    let listener = TcpListener::bind("127.0.0.1:8080").await.unwrap();
    println!("服务器启动在 127.0.0.1:8080");

    let game = Arc::new(Mutex::new(Game::new()));
    let user_manager = Arc::new(Mutex::new(UserManager::new()));

    // 处理 Ctrl+C 信号
    let game_clone = game.clone();
    tokio::spawn(async move {
        signal::ctrl_c().await.unwrap();
        game_clone.lock().await.shutdown().await;
        // 等待一小段时间确保消息被发送
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        std::process::exit(0);
    });

    while let Ok((stream, _)) = listener.accept().await {
        let game = game.clone();
        let user_manager = user_manager.clone();

        tokio::spawn(async move {
            handle_connection(stream, game, user_manager).await;
        });
    }
}

async fn handle_connection(
    stream: TcpStream,
    game: Arc<Mutex<Game>>,
    user_manager: Arc<Mutex<UserManager>>,
) {
    let ws_stream = accept_async(stream).await.unwrap();
    let (mut ws_sender, mut ws_receiver) = ws_stream.split();

    let (tx, mut rx) = mpsc::channel(32);

    // 等待客户端发送用户名
    let username = match ws_receiver.next().await {
        Some(Ok(Message::Text(text))) => {
            println!("收到连接消息: {}", text);
            if let Ok(GameMessage::Connect { username }) = serde_json::from_str(&text) {
                println!("新玩家 {} 正在连接...", username);
                username
            } else {
                println!("无效的连接消息格式");
                let _ = ws_sender
                    .send(Message::Text(
                        serde_json::to_string(&GameMessage::Error("无效的连接消息".to_string()))
                            .unwrap(),
                    ))
                    .await;
                return;
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
        let mut user_manager = user_manager.lock().await;
        let user = user_manager.create_user(username.clone());
        println!("创建用户: {:?}", user);
        user
    };

    // 获取当前游戏状态
    let mut game_guard = game.lock().await;
    let player = if !game_guard.players.contains_key(&PlayerRole::Black) {
        println!("分配玩家角色: Black");
        PlayerRole::Black
    } else if !game_guard.players.contains_key(&PlayerRole::White) {
        println!("分配玩家角色: White");
        PlayerRole::White
    } else {
        println!("游戏已满，拒绝连接");
        let _ = ws_sender
            .send(Message::Text(
                serde_json::to_string(&GameMessage::Error("游戏已满".to_string())).unwrap(),
            ))
            .await;
        return;
    };

    // 分配玩家角色给用户
    {
        let mut user_manager = user_manager.lock().await;
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
    if let Err(e) = game_guard.add_player(player, tx.clone()).await {
        println!("添加玩家到游戏失败: {}", e);
        let _ = ws_sender
            .send(Message::Text(
                serde_json::to_string(&GameMessage::Error(e.to_string())).unwrap(),
            ))
            .await;
        return;
    }
    println!("成功添加玩家 {} ({:?}) 到游戏", user.name, player);

    // 如果只有一个玩家，设置 AI 玩家
    if game_guard.players.len() == 1 {
        let ai_player = if player == PlayerRole::Black {
            PlayerRole::White
        } else {
            PlayerRole::Black
        };
        println!("设置 AI 玩家: {:?}", ai_player);
        game_guard.set_ai_player(ai_player);
    }

    // 如果游戏已满，移除 AI 玩家
    if game_guard.players.len() == 2 {
        println!("游戏已满，移除 AI 玩家");
        game_guard.remove_ai_player();
    }

    // 发送连接成功消息
    let _ = ws_sender
        .send(Message::Text(
            serde_json::to_string(&GameMessage::Connect {
                username: user.name.clone(),
            })
            .unwrap(),
        ))
        .await;
    println!("发送连接成功消息给玩家 {}", user.name);

    // 通知其他玩家有新玩家加入
    for (_, other_tx) in &game_guard.players {
        other_tx
            .send(GameMessage::PlayerConnected {
                player,
                username: user.name.clone(),
            })
            .await
            .unwrap();
    }
    println!("通知其他玩家 {} ({:?}) 已加入", user.name, player);

    drop(game_guard); // 释放锁

    // 处理游戏消息
    let game_clone = game.clone();
    let user_manager_clone = user_manager.clone();
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
