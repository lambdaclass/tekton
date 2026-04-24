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
        "ALTER TABLE tasks ADD COLUMN IF NOT EXISTS total_cost_usd DOUBLE PRECISION DEFAULT 0",
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
    for col_sql in &["ALTER TABLE task_messages ADD COLUMN IF NOT EXISTS image_url TEXT"] {
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

    // Add role column to users table
    let _ = sqlx::query(
        "ALTER TABLE users ADD COLUMN IF NOT EXISTS role TEXT NOT NULL DEFAULT 'member'",
    )
    .execute(pool)
    .await;

    // Ensure at least one admin exists — promote the earliest user if none
    let admin_count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM users WHERE role = 'admin'")
        .fetch_one(pool)
        .await?;
    if admin_count.0 == 0 {
        let _ = sqlx::query(
            "UPDATE users SET role = 'admin' WHERE github_login = (SELECT github_login FROM users ORDER BY created_at ASC LIMIT 1)",
        )
        .execute(pool)
        .await;
    }

    // Create user_repo_permissions table
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS user_repo_permissions (
            github_login TEXT NOT NULL REFERENCES users(github_login),
            repo TEXT NOT NULL,
            PRIMARY KEY (github_login, repo)
        )",
    )
    .execute(pool)
    .await?;

    // Create secrets table
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS secrets (
            id BIGSERIAL PRIMARY KEY,
            repo TEXT NOT NULL,
            name TEXT NOT NULL,
            encrypted_value TEXT NOT NULL,
            created_by TEXT,
            created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
            UNIQUE(repo, name)
        )",
    )
    .execute(pool)
    .await?;

    // Add ssh_public_key column to users table
    let _ = sqlx::query("ALTER TABLE users ADD COLUMN IF NOT EXISTS ssh_public_key TEXT")
        .execute(pool)
        .await;

    // Create repo_policies table
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS repo_policies (
            id BIGSERIAL PRIMARY KEY,
            repo TEXT NOT NULL UNIQUE,
            protected_branches TEXT[] NOT NULL DEFAULT '{main,master}',
            allowed_tools JSONB,
            network_egress JSONB,
            max_cost_usd DOUBLE PRECISION,
            require_approval_above_usd DOUBLE PRECISION,
            created_by TEXT,
            created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
            updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
        )",
    )
    .execute(pool)
    .await?;

    // Create org_policies table
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS org_policies (
            id BIGSERIAL PRIMARY KEY,
            org TEXT NOT NULL UNIQUE,
            protected_branches TEXT[] NOT NULL DEFAULT '{main,master}',
            allowed_tools JSONB,
            network_egress JSONB,
            max_cost_usd DOUBLE PRECISION,
            require_approval_above_usd DOUBLE PRECISION,
            created_by TEXT,
            created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
            updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
        )",
    )
    .execute(pool)
    .await?;

    // Add AI provider columns to users table
    for col_sql in &[
        "ALTER TABLE users ADD COLUMN IF NOT EXISTS ai_provider TEXT",
        "ALTER TABLE users ADD COLUMN IF NOT EXISTS ai_api_key_encrypted TEXT",
        "ALTER TABLE users ADD COLUMN IF NOT EXISTS ai_model TEXT",
    ] {
        let _ = sqlx::query(col_sql).execute(pool).await;
    }

    // Create audit_log table
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS audit_log (
            id BIGSERIAL PRIMARY KEY,
            event_type TEXT NOT NULL,
            actor TEXT NOT NULL,
            target TEXT,
            detail JSONB,
            ip_address TEXT,
            created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
        )",
    )
    .execute(pool)
    .await?;

    // Indexes for audit_log queries
    let _ = sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_audit_log_event_type_created \
         ON audit_log (event_type, created_at)",
    )
    .execute(pool)
    .await;
    let _ = sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_audit_log_actor_created \
         ON audit_log (actor, created_at)",
    )
    .execute(pool)
    .await;

    // Add compute_seconds column to tasks table
    let _ = sqlx::query("ALTER TABLE tasks ADD COLUMN IF NOT EXISTS compute_seconds INTEGER")
        .execute(pool)
        .await;

    // Create global_ai_config table (singleton – stores org-wide fallback AI key)
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS global_ai_config (
            id INTEGER PRIMARY KEY DEFAULT 1 CHECK (id = 1),
            ai_provider TEXT NOT NULL,
            ai_api_key_encrypted TEXT NOT NULL,
            ai_model TEXT,
            updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
            updated_by TEXT
        )",
    )
    .execute(pool)
    .await?;

    // Create budgets table
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS budgets (
            id BIGSERIAL PRIMARY KEY,
            scope TEXT NOT NULL,
            scope_type TEXT NOT NULL CHECK (scope_type IN ('user', 'org')),
            monthly_limit_usd DOUBLE PRECISION NOT NULL,
            alert_threshold_pct INTEGER NOT NULL DEFAULT 80,
            created_by TEXT,
            created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
            updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
            UNIQUE(scope, scope_type)
        )",
    )
    .execute(pool)
    .await?;

    // Add intake columns to tasks table
    for col_sql in &[
        "ALTER TABLE tasks ADD COLUMN IF NOT EXISTS intake_issue_id BIGINT",
        "ALTER TABLE tasks ADD COLUMN IF NOT EXISTS source_type TEXT DEFAULT 'manual'",
    ] {
        let _ = sqlx::query(col_sql).execute(pool).await;
    }

    // Create intake_sources table
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS intake_sources (
            id BIGSERIAL PRIMARY KEY,
            name TEXT NOT NULL,
            provider TEXT NOT NULL,
            enabled BOOLEAN NOT NULL DEFAULT true,
            config JSONB NOT NULL DEFAULT '{}',
            api_token_encrypted TEXT,
            target_repo TEXT NOT NULL,
            target_base_branch TEXT NOT NULL DEFAULT 'main',
            label_filter TEXT[] NOT NULL DEFAULT '{}',
            prompt_template TEXT,
            run_as_user TEXT NOT NULL,
            poll_interval_secs INTEGER NOT NULL DEFAULT 300,
            max_concurrent_tasks INTEGER NOT NULL DEFAULT 3,
            max_tasks_per_poll INTEGER NOT NULL DEFAULT 5,
            auto_create_pr BOOLEAN NOT NULL DEFAULT false,
            created_by TEXT NOT NULL,
            created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
            updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
        )",
    )
    .execute(pool)
    .await?;

    // Create intake_issues table
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS intake_issues (
            id BIGSERIAL PRIMARY KEY,
            source_id BIGINT NOT NULL REFERENCES intake_sources(id) ON DELETE CASCADE,
            external_id TEXT NOT NULL,
            external_url TEXT,
            external_title TEXT NOT NULL,
            external_body TEXT,
            external_labels TEXT[] NOT NULL DEFAULT '{}',
            external_updated_at TIMESTAMPTZ,
            task_id TEXT REFERENCES tasks(id),
            status TEXT NOT NULL DEFAULT 'backlog',
            error_message TEXT,
            prompt TEXT,
            created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
            updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
            UNIQUE(source_id, external_id)
        )",
    )
    .execute(pool)
    .await?;

    // Make api_token_encrypted nullable for existing installs (token now comes from users table)
    let _ =
        sqlx::query("ALTER TABLE intake_sources ALTER COLUMN api_token_encrypted DROP NOT NULL")
            .execute(pool)
            .await;

    // Add prompt column and migrate status defaults for existing installs
    for col_sql in &[
        "ALTER TABLE intake_issues ADD COLUMN IF NOT EXISTS prompt TEXT",
        "ALTER TABLE intake_issues ALTER COLUMN status SET DEFAULT 'backlog'",
    ] {
        let _ = sqlx::query(col_sql).execute(pool).await;
    }
    // Migrate orphan pending issues (no task spawned yet) to backlog
    let _ = sqlx::query(
        "UPDATE intake_issues SET status = 'backlog' WHERE status = 'pending' AND task_id IS NULL",
    )
    .execute(pool)
    .await;

    // Create intake_poll_log table
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS intake_poll_log (
            id BIGSERIAL PRIMARY KEY,
            source_id BIGINT NOT NULL REFERENCES intake_sources(id) ON DELETE CASCADE,
            polled_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
            issues_found INTEGER NOT NULL DEFAULT 0,
            issues_created INTEGER NOT NULL DEFAULT 0,
            issues_skipped INTEGER NOT NULL DEFAULT 0,
            error_message TEXT,
            duration_ms INTEGER
        )",
    )
    .execute(pool)
    .await?;

    Ok(())
}
