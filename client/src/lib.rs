use chess::{Board, GameMessage};
use futures_util::{SinkExt, StreamExt};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::WebSocketStream;

pub async fn handle_game_message(game_msg: GameMessage, board: &mut Board) -> bool {
    match game_msg {
        GameMessage::Connect { username } => {
            println!("\n已连接到游戏，欢迎 {}!", username);
            false
        }
        GameMessage::PlayerConnected { player, username } => {
            println!("玩家 {} ({:?}) 已加入游戏", username, player);
            false
        }
        GameMessage::ServerShutdown => {
            println!("服务器已关闭，游戏结束");
            true
        }
        GameMessage::Move { row, col } => {
            if let Err(e) = board.make_move(row, col) {
                println!("移动错误: {}", e);
            }
            false
        }
        GameMessage::Error(msg) => {
            println!("错误: {}", msg);
            false
        }
        GameMessage::GameOver { winner } => {
            match winner {
                Some(player) => {
                    println!("游戏结束！胜利者是: {:?}", player);
                }
                None => {
                    println!("游戏结束！平局！");
                }
            }
            true
        }
        GameMessage::Status {
            board: new_board,
            current_player,
        } => {
            println!("当前玩家: {:?}", current_player);
            board.cells = new_board;
            board.current_player = current_player;
            board.display();
            false
        }
        GameMessage::PlayerDisconnected { player } => {
            println!("玩家: {:?} 断开连接", player);
            false
        }
    }
}

pub async fn run_game(
    ws_stream: WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
    username: String,
) {
    let (write, mut read) = ws_stream.split();
    let write = Arc::new(Mutex::new(write));
    let (tx, rx) = mpsc::channel(32);
    let mut board = Board::new();
    let game_over = Arc::new(AtomicBool::new(false));

    // 发送用户名到服务器
    let connect_msg = GameMessage::Connect { username };
    let json = serde_json::to_string(&connect_msg).unwrap();
    println!("准备发送连接消息: {}", json);
    if let Err(e) = write.lock().await.send(Message::Text(json)).await {
        eprintln!("发送用户名失败: {}", e);
        return;
    }
    println!("连接消息发送成功");

    println!("欢迎来到井字棋游戏！");
    println!("等待服务器分配玩家角色...");

    // 处理消息发送的任务
    let write_task = {
        let write = write.clone();
        let mut rx = rx;
        tokio::spawn(async move {
            println!("write_task 已启动");
            loop {
                println!("write_task 等待消息...");
                match rx.recv().await {
                    Some(msg) => {
                        println!("从通道接收到消息: {:?}", msg);
                        // if let Err(e) = write.lock().await.send(msg).await {
                        //     eprintln!("发送消息失败: {}", e);
                        //     break;
                        // }
                        println!("消息发送到服务器成功");
                    }
                    None => {
                        println!("通道已关闭");
                        break;
                    }
                }
            }
            println!("write_task 正在关闭连接");
            // 关闭 WebSocket 连接
            let _ = write.lock().await.close().await;
            println!("write_task 已结束");
        })
    };

    let _ = tx.send(Message::Close(None)).await;

    // 处理用户输入的任务
    let tx_clone = tx.clone();
    let game_over_clone = game_over.clone();
    let input_task = tokio::spawn(async move {
        let stdin = std::io::stdin();
        let mut line = String::new();

        while !game_over_clone.load(Ordering::SeqCst) {
            line.clear();
            if let Err(e) = stdin.read_line(&mut line) {
                eprintln!("输入错误: {}", e);
                break;
            }

            let input = line.trim();
            if input.eq_ignore_ascii_case("quit") {
                game_over_clone.store(true, Ordering::SeqCst);
                // 发送关闭消息
                println!("准备发送关闭消息");
                let _ = tx_clone.send(Message::Close(None)).await;
                println!("关闭消息已发送到通道");
                println!("正在退出游戏...");
                // 等待一小段时间确保消息被发送
                tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                std::process::exit(0);
            }

            let parts: Vec<&str> = input.split_whitespace().collect();
            if parts.len() == 3 && parts[0].eq_ignore_ascii_case("move") {
                match (parts[1].parse::<usize>(), parts[2].parse::<usize>()) {
                    (Ok(row), Ok(col)) => {
                        let move_msg = GameMessage::Move { row, col };
                        let json = serde_json::to_string(&move_msg).unwrap();
                        println!("准备发送移动消息到通道: {}", json);
                        if let Err(e) = tx_clone.send(Message::Text(json)).await {
                            eprintln!("发送移动消息失败: {}", e);
                            game_over_clone.store(true, Ordering::SeqCst);
                            println!("连接已断开，正在退出...");
                            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                            std::process::exit(0);
                        }
                        println!("移动消息已发送到通道");
                    }
                    _ => println!("无效的行/列。用法: move <行> <列> (0-2)"),
                }
            } else {
                println!("无效的命令。用法: move <行> <列> (0-2)");
            }
        }
    });

    println!("输入格式: move <行> <列> (例如: move 0 1)");
    println!("输入 'quit' 退出游戏");

    // 处理接收消息
    while let Some(Ok(msg)) = read.next().await {
        println!("收到消息: {:?}", msg);
        if let Message::Text(text) = msg {
            match serde_json::from_str::<GameMessage>(&text) {
                Ok(game_msg) => {
                    println!("成功解析消息: {:?}", game_msg);
                    if handle_game_message(game_msg, &mut board).await {
                        game_over.store(true, Ordering::SeqCst);
                        // 发送关闭消息
                        println!("准备发送关闭消息");
                        let _ = tx.send(Message::Close(None)).await;
                        println!("关闭消息已发送到通道");
                        println!("游戏结束，正在退出...");
                        // 等待一小段时间确保消息被发送
                        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                        std::process::exit(0);
                    }
                }
                Err(e) => eprintln!("解析消息失败: {}", e),
            }
        }
    }

    // 等待任务完成
    let _ = write_task.await;
    let _ = input_task.await;
}
