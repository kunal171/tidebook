//! PostgreSQL pool creation and migration lifecycle.

use std::{error::Error, time::Duration};

use sqlx::{postgres::PgPoolOptions, PgPool};

/// Creates the shared PostgreSQL pool and applies pending migrations.
///
/// Migrations complete before Solana ingestion begins, preventing the indexer
/// from processing transactions without a compatible storage schema.
pub async fn connect(database_url: &str) -> Result<PgPool, Box<dyn Error + Send + Sync>> {
    let pool = PgPoolOptions::new()
        .max_connections(5)
        .acquire_timeout(Duration::from_secs(10))
        .connect(database_url)
        .await?;

    sqlx::migrate!("./migrations").run(&pool).await?;

    Ok(pool)
}
