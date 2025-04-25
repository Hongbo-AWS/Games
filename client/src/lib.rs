use chess::{Board, GameMessage};
use futures::{AsyncRead, AsyncWrite, Sink};
use futures_util::{SinkExt, StreamExt};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::io::{self, AsyncBufReadExt, BufReader};
use tokio::sync::Mutex;
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

pub async fn handle_user_input(
    write: &mut futures_util::stream::SplitSink<
        WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
        Message,
    >,
    game_over: &Arc<AtomicBool>,
) -> bool {
    let stdin = io::stdin();
    let mut reader = BufReader::new(stdin);
    let mut line = String::new();

    match reader.read_line(&mut line).await {
        Ok(0) => return true,
        Ok(_) => {
            let input = line.trim();
            if input.eq_ignore_ascii_case("quit") {
                game_over.store(true, Ordering::SeqCst);
                return true;
            }

            let parts: Vec<&str> = input.split_whitespace().collect();
            if parts.len() == 3 && parts[0].eq_ignore_ascii_case("move") {
                match (parts[1].parse::<usize>(), parts[2].parse::<usize>()) {
                    (Ok(row), Ok(col)) => {
                        let move_msg = GameMessage::Move { row, col };
                        let json = serde_json::to_string(&move_msg).unwrap();
                        println!("发送移动消息: {}", json);
                        if let Err(e) = write.send(Message::Text(json)).await {
                            eprintln!("发送消息失败: {}", e);
                            game_over.store(true, Ordering::SeqCst);
                            return true;
                        }
                    }
                    _ => println!("无效的行/列。用法: move <行> <列> (0-2)"),
                }
            } else {
                println!("无效的命令。用法: move <行> <列> (0-2)");
            }
        }
        Err(e) => {
            eprintln!("输入错误: {}", e);
            game_over.store(true, Ordering::SeqCst);
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
    let write = Arc::new(Mutex::new(write));
    let board = Arc::new(Mutex::new(Board::new()));
    let game_over = Arc::new(AtomicBool::new(false));

    // 发送用户名到服务器
    let connect_msg = GameMessage::Connect { username };
    let json = serde_json::to_string(&connect_msg).unwrap();
    if let Ok(mut write) = write.try_lock() {
        if let Err(e) = write.send(Message::Text(json)).await {
            eprintln!("发送用户名失败: {}", e);
            return;
        }
    }

    println!("欢迎来到井字棋游戏！");
    println!("等待服务器分配玩家角色...");

    // 处理接收消息的任务
    let write_clone = write.clone();
    let board_clone = board.clone();
    let game_over_clone = game_over.clone();
    let read_task = tokio::spawn(async move {
        println!("开始监听服务器消息...");
        while let Some(Ok(msg)) = read.next().await {
            if let Message::Text(text) = msg {
                println!("收到消息: {}", text);
                match serde_json::from_str::<GameMessage>(&text) {
                    Ok(game_msg) => {
                        println!("成功解析消息: {:?}", game_msg);
                        let mut board = board_clone.lock().await;
                        if handle_game_message(game_msg, &mut board).await {
                            // 游戏结束，关闭连接
                            if let Ok(mut write) = write_clone.try_lock() {
                                let _ = write.close().await;
                            }
                            game_over_clone.store(true, Ordering::SeqCst);
                            break;
                        }
                    }
                    Err(e) => eprintln!("解析消息失败: {}", e),
                }
            }
        }
    });

    println!("输入格式: move <行> <列> (例如: move 0 1)");
    println!("输入 'quit' 退出游戏");

    // 处理用户输入
    let write_clone = write.clone();
    let game_over_clone = game_over.clone();
    let input_task = tokio::spawn(async move {
        while !game_over_clone.load(Ordering::SeqCst) {
            if let Ok(mut write) = write_clone.try_lock() {
                if handle_user_input(&mut write, &game_over_clone).await {
                    break;
                }
            }
        }
    });

    // 创建一个专门的任务来检查游戏状态
    let game_over_clone = game_over.clone();
    let check_task = tokio::spawn(async move {
        while !game_over_clone.load(Ordering::SeqCst) {
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        }
        println!("游戏已结束，正在退出...");
        // 强制退出程序
        std::process::exit(0);
    });

    // 等待读取任务完成
    if let Err(e) = read_task.await {
        eprintln!("读取任务错误: {}", e);
    }

    // 等待输入任务完成
    if let Err(e) = input_task.await {
        eprintln!("输入任务错误: {}", e);
    }

    // 等待检查任务完成
    if let Err(e) = check_task.await {
        eprintln!("检查任务错误: {}", e);
    }
}
