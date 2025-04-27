use client::run_game;
use std::io;
use tokio_tungstenite::connect_async;

#[tokio::main]
async fn main() {
    let url = "ws://localhost:8080";
    println!("正在连接到服务器: {}", url);

    // 获取用户名
    println!("请输入您的用户名:");
    let mut username = String::new();
    io::stdin().read_line(&mut username).unwrap();
    let username = username.trim().to_string();

    match connect_async(url).await {
        Ok((ws_stream, _)) => {
            println!("已连接到服务器");
            run_game(ws_stream, username).await;
        }
        Err(e) => eprintln!("连接失败: {}", e),
    }
}
