use crate::GameError;
use crate::Player;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub id: String,             // 用户唯一标识
    pub name: String,           // 用户名
    pub session_id: String,     // 会话ID
    pub player: Option<Player>, // 当前游戏中的角色
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UserSession {
    pub user_id: String,
    pub session_id: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub expires_at: chrono::DateTime<chrono::Utc>,
}

pub struct UserManager {
    users: HashMap<String, User>,                // 用户ID -> 用户信息
    sessions: HashMap<String, UserSession>,      // 会话ID -> 会话信息
    player_assignments: HashMap<Player, String>, // 玩家 -> 用户ID
}

impl UserManager {
    pub fn new() -> Self {
        Self {
            users: HashMap::new(),
            sessions: HashMap::new(),
            player_assignments: HashMap::new(),
        }
    }

    pub fn create_user(&mut self, name: String) -> User {
        let user_id = uuid::Uuid::new_v4().to_string();
        let session_id = uuid::Uuid::new_v4().to_string();
        let user = User {
            id: user_id.clone(),
            name,
            session_id: session_id.clone(),
            player: None,
        };

        let session = UserSession {
            user_id: user_id.clone(),
            session_id,
            created_at: chrono::Utc::now(),
            expires_at: chrono::Utc::now() + chrono::Duration::hours(24),
        };

        self.users.insert(user_id.clone(), user.clone());
        self.sessions.insert(session.session_id.clone(), session);
        user
    }

    pub fn get_user_by_session(&self, session_id: &str) -> Option<&User> {
        self.sessions
            .get(session_id)
            .and_then(|session| self.users.get(&session.user_id))
    }

    pub fn assign_player(&mut self, user_id: &str, player: Player) -> Result<(), GameError> {
        if self.player_assignments.contains_key(&player) {
            return Err(GameError::InvalidInput(
                "Player already assigned".to_string(),
            ));
        }
        self.player_assignments.insert(player, user_id.to_string());
        if let Some(user) = self.users.get_mut(user_id) {
            user.player = Some(player);
        }
        Ok(())
    }

    pub fn get_user_by_player(&self, player: &Player) -> Option<&User> {
        self.player_assignments
            .get(player)
            .and_then(|user_id| self.users.get(user_id))
    }

    pub fn remove_user(&mut self, user_id: &str) {
        if let Some(user) = self.users.remove(user_id) {
            self.sessions.remove(&user.session_id);
            if let Some(player) = user.player {
                self.player_assignments.remove(&player);
            }
        }
    }
}
