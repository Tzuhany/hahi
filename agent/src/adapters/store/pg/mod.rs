// ============================================================================
// PostgreSQL — Warm/Durable Storage
//
// Agent owns four tables in PG:
//   checkpoints  — conversation snapshots
//   memories     — persistent memory + pgvector
//   tool_results — large tool outputs
//   audit_log    — execution audit trail
// ============================================================================

pub mod audit;
pub mod checkpoint;
pub mod memory;
pub mod runtime_state;
pub mod tool_result;

use crate::adapters::store::Store;

impl Store {
    pub(crate) fn pg(&self) -> &sqlx::PgPool {
        &self.pg
    }
}
