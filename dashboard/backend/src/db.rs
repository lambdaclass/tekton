use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::SqlitePool;
use std::str::FromStr;

pub async fn init_pool(database_url: &str) -> anyhow::Result<SqlitePool> {
    let opts = SqliteConnectOptions::from_str(database_url)?
        .create_if_missing(true)
        .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal);

    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(opts)
        .await?;

    run_migrations(&pool).await?;
    Ok(pool)
}

async fn run_migrations(pool: &SqlitePool) -> anyhow::Result<()> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS tasks (
            id TEXT PRIMARY KEY,
            prompt TEXT NOT NULL,
            repo TEXT NOT NULL,
            base_branch TEXT NOT NULL DEFAULT 'main',
            branch_name TEXT,
            agent_name TEXT,
            status TEXT NOT NULL DEFAULT 'pending',
            preview_slug TEXT,
            preview_url TEXT,
            error_message TEXT,
            created_at TEXT NOT NULL DEFAULT (datetime('now')),
            updated_at TEXT NOT NULL DEFAULT (datetime('now')),
            parent_task_id TEXT,
            created_by TEXT,
            screenshot_url TEXT,
            image_url TEXT,
            pr_url TEXT,
            pr_number INTEGER
        )",
    )
    .execute(pool)
    .await?;

    // Add new columns to existing tasks table if they don't exist (for upgrades)
    for col_sql in &[
        "ALTER TABLE tasks ADD COLUMN parent_task_id TEXT",
        "ALTER TABLE tasks ADD COLUMN created_by TEXT",
        "ALTER TABLE tasks ADD COLUMN screenshot_url TEXT",
        "ALTER TABLE tasks ADD COLUMN image_url TEXT",
        "ALTER TABLE tasks ADD COLUMN pr_url TEXT",
        "ALTER TABLE tasks ADD COLUMN pr_number INTEGER",
        "ALTER TABLE tasks ADD COLUMN name TEXT",
    ] {
        // Ignore errors — column likely already exists
        let _ = sqlx::query(col_sql).execute(pool).await;
    }

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS task_logs (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            task_id TEXT NOT NULL REFERENCES tasks(id),
            timestamp TEXT NOT NULL DEFAULT (datetime('now')),
            line TEXT NOT NULL
        )",
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS task_messages (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            task_id TEXT NOT NULL REFERENCES tasks(id),
            sender TEXT NOT NULL,
            content TEXT NOT NULL,
            created_at TEXT NOT NULL DEFAULT (datetime('now')),
            image_url TEXT
        )",
    )
    .execute(pool)
    .await?;

    // Add new columns to task_messages if they don't exist (for upgrades)
    for col_sql in &[
        "ALTER TABLE task_messages ADD COLUMN image_url TEXT",
    ] {
        let _ = sqlx::query(col_sql).execute(pool).await;
    }

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS users (
            github_login TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            email TEXT,
            github_token TEXT NOT NULL,
            created_at TEXT NOT NULL DEFAULT (datetime('now')),
            updated_at TEXT NOT NULL DEFAULT (datetime('now'))
        )",
    )
    .execute(pool)
    .await?;

    Ok(())
}
