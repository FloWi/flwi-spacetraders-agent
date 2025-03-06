// In a new file: st_core/src/model_manager.rs or similar
use sqlx::{Pool, Postgres};

#[derive(Clone, Debug)]
pub struct DbModelManager {
    pool: Pool<Postgres>,
}

impl DbModelManager {
    pub fn new(pool: Pool<Postgres>) -> Self {
        Self { pool }
    }

    pub fn pool(&self) -> &Pool<Postgres> {
        &self.pool
    }
}
