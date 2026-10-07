use std::path::Path;

use sqlx::sqlite::{SqliteConnectOptions, SqlitePool, SqlitePoolOptions};
use sqlx::{ConnectOptions, Transaction};

use crate::error::{AppError, AppResult};

pub async fn connect(db_path: &Path) -> AppResult<SqlitePool> {
    let url = format!("sqlite://{}?mode=rwc", db_path.display());
    let options: SqliteConnectOptions = url
        .parse::<SqliteConnectOptions>()?
        .foreign_keys(true)
        .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
        .busy_timeout(std::time::Duration::from_millis(5000))
        .create_if_missing(true);
    // Quiet sqlx connect logging.
    let options = options.disable_statement_logging();
    let pool = SqlitePoolOptions::new()
        .max_connections(8)
        .connect_with(options)
        .await?;
    Ok(pool)
}

pub async fn migrate(pool: &SqlitePool) -> AppResult<()> {
    sqlx::migrate!("./migrations").run(pool).await.map_err(AppError::internal)?;
    Ok(())
}

/// Begin a `BEGIN IMMEDIATE` transaction. Use for every check-then-write
/// sequence (booking confirmation, payment recording, submission, ...) so a
/// concurrent writer fails fast instead of racing past the check.
pub async fn begin_immediate(pool: &SqlitePool) -> AppResult<Transaction<'_, sqlx::Sqlite>> {
    Ok(pool.begin_with("BEGIN IMMEDIATE").await?)
}
