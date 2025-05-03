use chess::{Board, GameMessage, PlayerRole};
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
    let player_role = Arc::new(Mutex::new(None::<PlayerRole>));

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

    // 处理接收消息的任务
    let board_clone = board.clone();
    let player_role_clone = player_role.clone();
    let read_task = {
        let game_over_sender = game_over_sender.clone();
        let mut game_over_receiver = game_over_sender.subscribe();
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
                                            match game_msg {
                                                GameMessage::ConnectResponse { username, player_role: role } => {
                                                    println!("收到连接响应: {} 被分配为 {:?}", username, role);
                                                    *player_role_clone.lock().await = Some(role);
                                                }
                                                _ => {
                                                    let mut board = board_clone.lock().await;
                                                    if handle_game_message(game_msg, &mut board).await {
                                                        println!("游戏结束，关闭读取任务");
                                                        let _ = game_over_sender.send(());
                                                        break;
                                                    }
                                                }
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

    // 处理 AI 移动的任务
    let ai_task = {
        let board = board.clone();
        let tx = tx.clone();
        let game_over_sender = game_over_sender.clone();
        let player_role = player_role.clone();
        let mut game_over_receiver = game_over_sender.subscribe();
        tokio::spawn(async move {
            let mut rng = StdRng::from_entropy();
            loop {
                tokio::select! {
                    _ = game_over_receiver.recv() => {
                        println!("游戏结束标志触发，AI 任务退出");
                        break;
                    }
                    _ = async {
                        // 等待一段时间，模拟 AI 思考
                        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;

                        let board = board.lock().await;
                        let player_role = player_role.lock().await;
                        if player_role.is_none() {
                            return;
                        }

                        // TODO: 实现 AI 逻辑，选择最佳移动
                        // 这里简单实现一个随机移动
                        let row = rng.gen_range(0..15);
                        let col = rng.gen_range(0..15);

                        let move_msg = GameMessage::Move { row, col };
                        let json = serde_json::to_string(&move_msg).unwrap();
                        println!("AI 发送移动消息: {}", json);
                        if let Err(e) = tx.send(Message::Text(json)).await {
                            eprintln!("发送消息失败: {}", e);
                            let _ = game_over_sender.send(());
                        }
                    } => {}
                }
            }
            println!("AI 任务结束");
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
