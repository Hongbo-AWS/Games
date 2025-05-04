use chess::{Board, GameMessage, PlayerRole};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::io::{self, AsyncBufReadExt, BufReader};
use tokio::sync::Mutex;
use tokio::sync::{broadcast, mpsc};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::WebSocketStream;

pub async fn handle_game_message(msg: GameMessage, board: &mut Board) -> bool {
    match msg {
        GameMessage::ConnectRequest { username } => {
            println!("\n正在连接到游戏，用户名: {}...", username);
            false
        }
        GameMessage::ConnectResponse {
            username,
            player_role,
        } => {
            println!(
                "\n已连接到游戏，欢迎 {}! 你的角色是: {:?}",
                username, player_role
            );
            false
        }
        GameMessage::Move { row, col } => {
            if let Err(e) = board.make_move(row, col) {
                println!("移动失败: {}", e);
            } else {
                board.display();
            }
            false
        }
        GameMessage::Error(msg) => {
            println!("\n错误: {}", msg);
            false
        }
        GameMessage::GameOver { winner } => {
            match winner {
                Some(role) => println!("\n游戏结束！胜利者是: {:?}", role),
                None => println!("\n游戏结束！平局！"),
            }
            true
        }
        GameMessage::Status {
            board: new_board,
            current_player,
        } => {
            board.cells = new_board;
            board.current_player = current_player;
            board.display();
            false
        }
        GameMessage::TurnNotification { player } => {
            println!("\n轮到玩家 {:?} 移动", player);
            false
        }
        GameMessage::PlayerDisconnected { player } => {
            println!("\n玩家 {:?} 已断开连接", player);
            false
        }
        GameMessage::PlayerConnected { player, username } => {
            println!("\n玩家 {} ({:?}) 已加入游戏", username, player);
            false
        }
        GameMessage::ServerShutdown => {
            println!("\n服务器已关闭");
            true
        }
    }
}

pub async fn handle_user_input(tx: &mpsc::Sender<Message>) -> bool {
    let stdin = io::stdin();
    let mut reader = BufReader::new(stdin);
    let mut line = String::new();

    // 等待用户输入
    let result = reader.read_line(&mut line).await;

    match result {
        Ok(0) => return true,
        Ok(_) => {
            let input = line.trim();
            if input.eq_ignore_ascii_case("quit") {
                return true;
            }

            let parts: Vec<&str> = input.split_whitespace().collect();
            if parts.len() == 3 && parts[0].eq_ignore_ascii_case("move") {
                match (parts[1].parse::<usize>(), parts[2].parse::<usize>()) {
                    (Ok(row), Ok(col)) => {
                        let move_msg = GameMessage::Move { row, col };
                        let json = serde_json::to_string(&move_msg).unwrap();
                        println!("发送移动消息: {}", json);
                        if let Err(e) = tx.send(Message::Text(json)).await {
                            eprintln!("发送消息失败: {}", e);
                            return true;
                        }
                    }
                    _ => println!("无效的行/列。用法: move <行> <列> (0-14)"),
                }
            } else {
                println!("无效的命令。用法: move <行> <列> (0-14)");
            }
        }
        Err(e) => {
            eprintln!("输入错误: {}", e);
            return true;
        }
    }

    false
}

pub async fn run_game(
    ws_stream: WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
    username: String,
) {
    let (mut write, mut read) = ws_stream.split();
    let (tx, mut rx) = mpsc::channel::<Message>(32);
    let board = Arc::new(Mutex::new(Board::new()));

    let (game_over_sender, _) = broadcast::channel::<()>(16);

    // 发送用户名到服务器
    let connect_msg = GameMessage::ConnectRequest { username };
    let json = serde_json::to_string(&connect_msg).unwrap();
    if let Err(e) = write.send(Message::Text(json)).await {
        eprintln!("发送用户名失败: {}", e);
        return;
    }

    println!("欢迎来到五子棋游戏！");
    println!("等待服务器分配玩家角色...");

    // 处理接收消息的任务
    let board_clone = board.clone();
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
                                            let mut board = board_clone.lock().await;
                                            if handle_game_message(game_msg, &mut board).await {
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

    println!("输入格式: move <行> <列> (例如: move 7 7)");
    println!("输入 'quit' 退出游戏");

    // 处理用户输入
    let tx_clone = tx.clone();

    let input_task = {
        let game_over_sender = game_over_sender.clone();
        let mut game_over_receiver = game_over_sender.subscribe();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = game_over_receiver.recv() => {
                        println!("游戏结束标志触发，输入任务退出");
                        break;
                    }
                    game_over = handle_user_input(&tx_clone) => {
                        println!("游戏结束，关闭输入任务");
                        if game_over {
                            let _ = game_over_sender.send(());
                            break;
                        }
                    }
                }
            }
            println!("输入任务结束");
        })
    };

    // 等待所有任务完成
    let (read_result, write_result, input_result) = tokio::join!(read_task, write_task, input_task);

    // 检查任务结果
    if let Err(e) = read_result {
        println!("读取任务错误: {}", e);
    }
    if let Err(e) = write_result {
        println!("写入任务错误: {}", e);
    }
    if let Err(e) = input_result {
        println!("输入任务错误: {}", e);
    }

    println!("\n游戏已结束，按回车键退出...");
    let mut input = String::new();
    let _ = std::io::stdin().read_line(&mut input);
}
