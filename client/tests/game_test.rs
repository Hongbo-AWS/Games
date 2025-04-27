use chess::{Board, GameMessage, PlayerRole};
use client::handle_game_message;
use client::handle_user_input;
use futures_util::StreamExt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::net::TcpStream;
use tokio::sync::mpsc::channel;
use tokio_tungstenite::{tungstenite::Message, MaybeTlsStream, WebSocketStream};

#[tokio::test]
async fn test_game_over_handling() {
    // 模拟游戏结束消息
    let game_over_msg = GameMessage::GameOver {
        winner: Some(PlayerRole::Black),
    };
    let mut board = Board::new();
    let game_over = Arc::new(AtomicBool::new(false));

    // 测试处理游戏结束消息
    let result = handle_game_message(game_over_msg, &mut board).await;
    assert!(result); // 应该返回 true 表示游戏结束
}

#[tokio::test]
async fn test_invalid_move() {
    // 模拟无效移动消息
    let move_msg = GameMessage::Move { row: 3, col: 3 }; // 超出范围
    let mut board = Board::new();
    let game_over = Arc::new(AtomicBool::new(false));

    // 测试处理无效移动
    let result = handle_game_message(move_msg, &mut board).await;
    assert!(!result); // 应该返回 false 表示游戏继续
}

#[tokio::test]
async fn test_player_quit() {
    let game_over = Arc::new(AtomicBool::new(false));
    let (tx, _rx) = tokio::sync::mpsc::channel::<Message>(32);
    let result = handle_user_input(&tx, &game_over).await;
    assert!(result);
}
