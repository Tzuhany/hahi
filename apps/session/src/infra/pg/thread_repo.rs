use anyhow::{Context, Result};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::PgPool;

use crate::domain::{Thread, ThreadId};
use crate::ports::repository::ThreadRepo;

pub struct PgThreadRepo {
    pool: PgPool,
}

impl PgThreadRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

// Row struct for mapping SELECT results.
#[derive(sqlx::FromRow)]
struct ThreadRow {
    id: String,
    user_id: String,
    title: String,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl From<ThreadRow> for Thread {
    fn from(r: ThreadRow) -> Self {
        Thread {
            id: ThreadId::from(r.id),
            user_id: r.user_id,
            title: r.title,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }
    }
}

#[async_trait]
impl ThreadRepo for PgThreadRepo {
    async fn insert(&self, thread: &Thread) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO threads (id, user_id, title, created_at, updated_at)
            VALUES ($1, $2, $3, $4, $5)
            "#,
        )
        .bind(thread.id.as_str())
        .bind(&thread.user_id)
        .bind(&thread.title)
        .bind(thread.created_at)
        .bind(thread.updated_at)
        .execute(&self.pool)
        .await
        .context("failed to insert thread")?;
        Ok(())
    }

    async fn find_by_id(&self, id: &ThreadId) -> Result<Option<Thread>> {
        let row: Option<ThreadRow> = sqlx::query_as(
            "SELECT id, user_id, title, created_at, updated_at FROM threads WHERE id = $1",
        )
        .bind(id.as_str())
        .fetch_optional(&self.pool)
        .await
        .context("failed to fetch thread")?;

        Ok(row.map(Thread::from))
    }

    async fn list_by_user(&self, user_id: &str, limit: i64, offset: i64) -> Result<Vec<Thread>> {
        let rows: Vec<ThreadRow> = sqlx::query_as(
            r#"
            SELECT id, user_id, title, created_at, updated_at
            FROM threads
            WHERE user_id = $1
            ORDER BY updated_at DESC
            LIMIT $2 OFFSET $3
            "#,
        )
        .bind(user_id)
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await
        .context("failed to list threads")?;

        Ok(rows.into_iter().map(Thread::from).collect())
    }

    async fn delete(&self, id: &ThreadId) -> Result<()> {
        sqlx::query("DELETE FROM threads WHERE id = $1")
            .bind(id.as_str())
            .execute(&self.pool)
            .await
            .context("failed to delete thread")?;
        Ok(())
    }
}
