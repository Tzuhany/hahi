use anyhow::{Context, Result};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::PgPool;

use crate::domain::{Message, MessageId, MessageRole, ThreadId};
use crate::ports::repository::MessageRepo;

pub struct PgMessageRepo {
    pool: PgPool,
}

impl PgMessageRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[derive(sqlx::FromRow)]
struct MessageRow {
    id: String,
    thread_id: String,
    role: String,
    content: String,
    created_at: DateTime<Utc>,
}

impl From<MessageRow> for Message {
    fn from(r: MessageRow) -> Self {
        Message {
            id: MessageId::from(r.id),
            thread_id: ThreadId::from(r.thread_id),
            role: parse_role(&r.role),
            content: r.content,
            created_at: r.created_at,
        }
    }
}

#[async_trait]
impl MessageRepo for PgMessageRepo {
    async fn insert(&self, msg: &Message) -> Result<()> {
        sqlx::query(
            "INSERT INTO messages (id, thread_id, role, content, created_at) VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(msg.id.as_str())
        .bind(msg.thread_id.as_str())
        .bind(msg.role.to_string())
        .bind(&msg.content)
        .bind(msg.created_at)
        .execute(&self.pool)
        .await
        .context("failed to insert message")?;
        Ok(())
    }

    async fn list_by_thread(
        &self,
        thread_id: &ThreadId,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<Message>> {
        let rows: Vec<MessageRow> = sqlx::query_as(
            r#"
            SELECT id, thread_id, role, content, created_at
            FROM messages
            WHERE thread_id = $1
            ORDER BY created_at ASC
            LIMIT $2 OFFSET $3
            "#,
        )
        .bind(thread_id.as_str())
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await
        .context("failed to list messages")?;

        Ok(rows.into_iter().map(Message::from).collect())
    }
}

fn parse_role(s: &str) -> MessageRole {
    match s {
        "assistant" => MessageRole::Assistant,
        _ => MessageRole::User,
    }
}
