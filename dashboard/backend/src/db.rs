use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;

pub async fn init_pool(database_url: &str) -> anyhow::Result<PgPool> {
    let pool = PgPoolOptions::new()
        .max_connections(10)
        .connect(database_url)
        .await?;

    run_migrations(&pool).await?;
    Ok(pool)
}

async fn run_migrations(pool: &PgPool) -> anyhow::Result<()> {
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
            created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
            updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
            parent_task_id TEXT,
            created_by TEXT,
            screenshot_url TEXT,
            image_url TEXT,
            total_input_tokens BIGINT DEFAULT 0,
            total_output_tokens BIGINT DEFAULT 0
        )",
    )
    .execute(pool)
    .await?;

    // Add new columns to existing tasks table if they don't exist (for upgrades)
    for col_sql in &[
        "ALTER TABLE tasks ADD COLUMN IF NOT EXISTS parent_task_id TEXT",
        "ALTER TABLE tasks ADD COLUMN IF NOT EXISTS created_by TEXT",
        "ALTER TABLE tasks ADD COLUMN IF NOT EXISTS screenshot_url TEXT",
        "ALTER TABLE tasks ADD COLUMN IF NOT EXISTS image_url TEXT",
        "ALTER TABLE tasks ADD COLUMN IF NOT EXISTS total_input_tokens BIGINT DEFAULT 0",
        "ALTER TABLE tasks ADD COLUMN IF NOT EXISTS total_output_tokens BIGINT DEFAULT 0",
        "ALTER TABLE tasks ADD COLUMN IF NOT EXISTS name TEXT",
        "ALTER TABLE tasks ADD COLUMN IF NOT EXISTS pr_url TEXT",
        "ALTER TABLE tasks ADD COLUMN IF NOT EXISTS pr_number INTEGER",
    ] {
        let _ = sqlx::query(col_sql).execute(pool).await;
    }

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS task_logs (
            id BIGSERIAL PRIMARY KEY,
            task_id TEXT NOT NULL REFERENCES tasks(id),
            timestamp TIMESTAMPTZ NOT NULL DEFAULT NOW(),
            line TEXT NOT NULL
        )",
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS task_messages (
            id BIGSERIAL PRIMARY KEY,
            task_id TEXT NOT NULL REFERENCES tasks(id),
            sender TEXT NOT NULL,
            content TEXT NOT NULL,
            created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
            image_url TEXT
        )",
    )
    .execute(pool)
    .await?;

    // Add new columns to task_messages if they don't exist (for upgrades)
    for col_sql in &[
        "ALTER TABLE task_messages ADD COLUMN IF NOT EXISTS image_url TEXT",
    ] {
        let _ = sqlx::query(col_sql).execute(pool).await;
    }

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS users (
            github_login TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            email TEXT,
            github_token TEXT NOT NULL,
            created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
            updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
        )",
    )
    .execute(pool)
    .await?;

    // Add Claude OAuth columns to users table
    for col_sql in &[
        "ALTER TABLE users ADD COLUMN IF NOT EXISTS claude_access_token TEXT",
        "ALTER TABLE users ADD COLUMN IF NOT EXISTS claude_refresh_token TEXT",
        "ALTER TABLE users ADD COLUMN IF NOT EXISTS claude_token_expires_at BIGINT",
    ] {
        let _ = sqlx::query(col_sql).execute(pool).await;
    }

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS claude_oauth_states (
            state TEXT PRIMARY KEY,
            github_login TEXT NOT NULL,
            code_verifier TEXT NOT NULL,
            created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
        )",
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS task_actions (
            id BIGSERIAL PRIMARY KEY,
            task_id TEXT NOT NULL REFERENCES tasks(id),
            action_type TEXT NOT NULL,
            tool_name TEXT,
            tool_input JSONB,
            summary TEXT,
            created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
        )",
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS task_state_transitions (
            id BIGSERIAL PRIMARY KEY,
            task_id TEXT NOT NULL REFERENCES tasks(id),
            from_status TEXT,
            to_status TEXT NOT NULL,
            created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
        )",
    )
    .execute(pool)
    .await?;

    Ok(())
}
