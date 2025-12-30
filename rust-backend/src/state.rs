use std::sync::Arc;

use sqlx::PgPool;
use tokio::sync::Mutex;

use crate::config::Config;
use crate::vision::VisionAnalyzer;

#[derive(Clone)]
pub struct AppState {
    pub db: PgPool,
    pub config: Config,
    pub vision: Option<Arc<Mutex<VisionAnalyzer>>>,
}
