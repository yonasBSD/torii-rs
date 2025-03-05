use async_trait::async_trait;
use chrono::{DateTime, Utc};
use torii_core::{
    UserId,
    error::StorageError,
    storage::{MagicLinkStorage, MagicToken},
};

use crate::PostgresStorage;

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct PostgresMagicToken {
    pub id: Option<String>,
    pub user_id: String,
    pub token: String,
    pub expires_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<PostgresMagicToken> for MagicToken {
    fn from(row: PostgresMagicToken) -> Self {
        MagicToken::new(
            UserId::new(&row.user_id),
            row.token.clone(),
            row.expires_at,
            row.created_at,
            row.updated_at,
        )
    }
}

impl From<&MagicToken> for PostgresMagicToken {
    fn from(token: &MagicToken) -> Self {
        PostgresMagicToken {
            id: None,
            user_id: token.user_id.as_str().to_string(),
            token: token.token.clone(),
            expires_at: token.expires_at,
            created_at: token.created_at,
            updated_at: token.updated_at,
        }
    }
}

#[async_trait]
impl MagicLinkStorage for PostgresStorage {
    async fn save_magic_token(&self, token: &MagicToken) -> Result<(), Self::Error> {
        let row = PostgresMagicToken::from(token);

        sqlx::query("INSERT INTO magic_links (user_id, token, expires_at, created_at, updated_at) VALUES ($1::uuid, $2, $3, $4, $5)")
            .bind(row.user_id)
            .bind(row.token)
            .bind(row.expires_at)
            .bind(row.created_at)
            .bind(row.updated_at)
            .execute(&self.pool)
            .await
            .map_err(|e| StorageError::Database(e.to_string()))?;

        Ok(())
    }

    async fn get_magic_token(&self, token: &str) -> Result<Option<MagicToken>, Self::Error> {
        let row: Option<PostgresMagicToken> =
            sqlx::query_as("SELECT id::text, user_id::text, token, expires_at, created_at, updated_at FROM magic_links WHERE token = $1")
                .bind(token)
                .fetch_optional(&self.pool)
                .await
                .map_err(|e| StorageError::Database(e.to_string()))?;

        Ok(row.map(|row| row.into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use chrono::Duration;
    use torii_core::{NewUser, User, UserStorage, storage::MagicLinkStorage};
    use uuid::Uuid;

    use crate::PostgresStorage;

    async fn create_test_user(storage: &PostgresStorage) -> User {
        let user = NewUser::builder()
            .email("test@test.com".to_string())
            .build()
            .expect("Failed to build test user");
        storage
            .create_user(&user)
            .await
            .expect("Failed to create test user")
    }

    #[tokio::test]
    async fn test_save_and_get_magic_token() {
        let storage = crate::tests::setup_test_db().await;

        // Create a user
        let user = create_test_user(&storage).await;

        let token = MagicToken::new(
            UserId::new(&user.id.to_string()),
            Uuid::new_v4().to_string(),
            Utc::now() + Duration::minutes(5),
            Utc::now(),
            Utc::now(),
        );
        storage
            .save_magic_token(&token)
            .await
            .expect("Failed to save magic token");

        let stored_token = storage
            .get_magic_token(&token.token)
            .await
            .expect("Failed to get magic token");
        assert!(stored_token.is_some());

        let stored_token = stored_token.unwrap();
        assert_eq!(stored_token.user_id, user.id);
        assert_eq!(stored_token.token, token.token);
        assert_eq!(stored_token.expires_at, token.expires_at);
    }

    #[tokio::test]
    async fn test_get_nonexistent_magic_token() {
        let storage = crate::tests::setup_test_db().await;

        let token = Uuid::new_v4().to_string();
        let result = storage
            .get_magic_token(&token)
            .await
            .expect("Failed to query magic token");
        assert!(result.is_none());
    }
}
