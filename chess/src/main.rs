use chess::{Game, NetworkPlayer, UserManager};

use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::signal;
use tokio::sync::Mutex;

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
            let network_player = NetworkPlayer::new(stream, game, user_manager);
            network_player.play().await;
        });
    }
}
