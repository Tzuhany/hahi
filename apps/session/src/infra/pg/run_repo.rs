use anyhow::{Context, Result};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::PgPool;

use crate::domain::{MessageId, Run, RunId, RunStatus, ThreadId};
use crate::ports::repository::RunRepo;

pub struct PgRunRepo {
    pool: PgPool,
}

impl PgRunRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[derive(sqlx::FromRow)]
struct RunRow {
    id: String,
    thread_id: String,
    status: String,
    agent_id: Option<String>,
    started_at: Option<DateTime<Utc>>,
    ended_at: Option<DateTime<Utc>>,
    result_message_id: Option<String>,
    failure_reason: Option<String>,
    created_at: DateTime<Utc>,
}

impl From<RunRow> for Run {
    fn from(r: RunRow) -> Self {
        Run {
            id: RunId::from(r.id),
            thread_id: ThreadId::from(r.thread_id),
            status: parse_status(&r.status),
            agent_id: r.agent_id,
            started_at: r.started_at,
            ended_at: r.ended_at,
            accumulated_content: String::new(),
            result_message_id: r.result_message_id.map(MessageId::from),
            failure_reason: r.failure_reason,
            created_at: r.created_at,
        }
    }
}

const SELECT_FIELDS: &str = "id, thread_id, status, agent_id, started_at, ended_at, result_message_id, failure_reason, created_at";

#[async_trait]
impl RunRepo for PgRunRepo {
    async fn insert(&self, run: &Run) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO runs
              (id, thread_id, status, agent_id,
               started_at, ended_at, result_message_id, failure_reason, created_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
            "#,
        )
        .bind(run.id.as_str())
        .bind(run.thread_id.as_str())
        .bind(run.status.to_string())
        .bind(run.agent_id.as_deref())
        .bind(run.started_at)
        .bind(run.ended_at)
        .bind(run.result_message_id.as_ref().map(|id| id.as_str()))
        .bind(run.failure_reason.as_deref())
        .bind(run.created_at)
        .execute(&self.pool)
        .await
        .context("failed to insert run")?;
        Ok(())
    }

    async fn update(&self, run: &Run) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE runs SET
              status            = $2,
              agent_id          = $3,
              started_at        = $4,
              ended_at          = $5,
              result_message_id = $6,
              failure_reason    = $7
            WHERE id = $1
            "#,
        )
        .bind(run.id.as_str())
        .bind(run.status.to_string())
        .bind(run.agent_id.as_deref())
        .bind(run.started_at)
        .bind(run.ended_at)
        .bind(run.result_message_id.as_ref().map(|id| id.as_str()))
        .bind(run.failure_reason.as_deref())
        .execute(&self.pool)
        .await
        .context("failed to update run")?;
        Ok(())
    }

    async fn find_by_id(&self, id: &RunId) -> Result<Option<Run>> {
        let row: Option<RunRow> =
            sqlx::query_as(&format!("SELECT {SELECT_FIELDS} FROM runs WHERE id = $1"))
                .bind(id.as_str())
                .fetch_optional(&self.pool)
                .await
                .context("failed to fetch run")?;

        Ok(row.map(Run::from))
    }

    async fn find_latest_by_thread(&self, thread_id: &ThreadId) -> Result<Option<Run>> {
        let row: Option<RunRow> = sqlx::query_as(&format!(
            "SELECT {SELECT_FIELDS} FROM runs WHERE thread_id = $1 ORDER BY created_at DESC LIMIT 1"
        ))
        .bind(thread_id.as_str())
        .fetch_optional(&self.pool)
        .await
        .context("failed to fetch latest run by thread")?;

        Ok(row.map(Run::from))
    }

    async fn find_by_status(&self, status: &RunStatus) -> Result<Vec<Run>> {
        let rows: Vec<RunRow> = sqlx::query_as(&format!(
            "SELECT {SELECT_FIELDS} FROM runs WHERE status = $1"
        ))
        .bind(status.to_string())
        .fetch_all(&self.pool)
        .await
        .context("failed to fetch runs by status")?;

        Ok(rows.into_iter().map(Run::from).collect())
    }
}

fn parse_status(s: &str) -> RunStatus {
    match s {
        "running" => RunStatus::Running,
        "completing" => RunStatus::Completing,
        "completed" => RunStatus::Completed,
        "failed" => RunStatus::Failed,
        "interrupted" => RunStatus::Interrupted,
        _ => RunStatus::Pending,
    }
}
