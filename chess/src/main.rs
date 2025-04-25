use chess::{Board, GameError, GameMessage, Player, UserManager};
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
    players: HashMap<Player, mpsc::Sender<GameMessage>>,
}

impl Game {
    fn new() -> Self {
        Game {
            board: Board::new(),
            players: HashMap::new(),
        }
    }

    async fn add_player(
        &mut self,
        player: Player,
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

    async fn make_move(&mut self, player: Player, row: usize, col: usize) -> Result<(), GameError> {
        if self.players.len() < 2 {
            return Err(GameError::InvalidInput("等待另一个玩家加入".to_string()));
        }
        if self.board.current_player != player {
            return Err(GameError::InvalidInput("不是你的回合".to_string()));
        }
        self.board.make_move(row, col)?;

        // 通知所有玩家移动和新的游戏状态
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

    async fn remove_player(&mut self, player: Player) {
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
        println!("服务器正在关闭...");
        let mut game = game_clone.lock().await;
        // 通知所有玩家服务器关闭
        for (_, tx) in &game.players {
            let _ = tx.send(GameMessage::ServerShutdown).await;
        }
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
            if let Ok(GameMessage::Connect { username }) = serde_json::from_str(&text) {
                username
            } else {
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
        user_manager.create_user(username.clone())
    };

    // 获取当前游戏状态
    let mut game_guard = game.lock().await;
    let player = if !game_guard.players.contains_key(&Player::X) {
        Player::X
    } else if !game_guard.players.contains_key(&Player::O) {
        Player::O
    } else {
        // 如果两个玩家都在，拒绝连接
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
            let _ = ws_sender
                .send(Message::Text(
                    serde_json::to_string(&GameMessage::Error(e.to_string())).unwrap(),
                ))
                .await;
            return;
        }
    }

    // 添加玩家到游戏
    if let Err(e) = game_guard.add_player(player, tx.clone()).await {
        let _ = ws_sender
            .send(Message::Text(
                serde_json::to_string(&GameMessage::Error(e.to_string())).unwrap(),
            ))
            .await;
        return;
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

    drop(game_guard); // 释放锁

    // 处理游戏消息
    let game_clone = game.clone();
    let user_manager_clone = user_manager.clone();
    tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            let _ = ws_sender
                .send(Message::Text(serde_json::to_string(&msg).unwrap()))
                .await;
        }
    });

    // 接收玩家移动
    while let Some(Ok(msg)) = ws_receiver.next().await {
        if let Message::Text(text) = msg {
            if let Ok(GameMessage::Move { row, col }) = serde_json::from_str(&text) {
                let mut game = game_clone.lock().await;
                if let Err(e) = game.make_move(player, row, col).await {
                    tx.send(GameMessage::Error(e.to_string())).await.unwrap();
                }
            }
        }
    }

    // 处理断开连接
    {
        let mut game = game_clone.lock().await;
        game.remove_player(player).await;
        let mut user_manager = user_manager_clone.lock().await;
        user_manager.remove_user(&user.id);
    }
}
