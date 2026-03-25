-- seed.sql: Seed data for e2e tests
-- Schema must match exactly what dashboard/backend/src/db.rs creates

-- ============================================================
-- Tables
-- ============================================================

CREATE TABLE IF NOT EXISTS tasks (
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
    total_output_tokens BIGINT DEFAULT 0,
    name TEXT,
    pr_url TEXT,
    pr_number INTEGER,
    total_cost_usd DOUBLE PRECISION DEFAULT 0,
    compute_seconds INTEGER
);

CREATE TABLE IF NOT EXISTS task_logs (
    id BIGSERIAL PRIMARY KEY,
    task_id TEXT NOT NULL REFERENCES tasks(id),
    timestamp TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    line TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS task_messages (
    id BIGSERIAL PRIMARY KEY,
    task_id TEXT NOT NULL REFERENCES tasks(id),
    sender TEXT NOT NULL,
    content TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    image_url TEXT
);

CREATE TABLE IF NOT EXISTS users (
    github_login TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    email TEXT,
    github_token TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    role TEXT NOT NULL DEFAULT 'member',
    ssh_public_key TEXT,
    ai_provider TEXT,
    ai_api_key_encrypted TEXT,
    ai_model TEXT
);

CREATE TABLE IF NOT EXISTS task_actions (
    id BIGSERIAL PRIMARY KEY,
    task_id TEXT NOT NULL REFERENCES tasks(id),
    action_type TEXT NOT NULL,
    tool_name TEXT,
    tool_input JSONB,
    summary TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS task_state_transitions (
    id BIGSERIAL PRIMARY KEY,
    task_id TEXT NOT NULL REFERENCES tasks(id),
    from_status TEXT,
    to_status TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS user_repo_permissions (
    github_login TEXT NOT NULL REFERENCES users(github_login),
    repo TEXT NOT NULL,
    PRIMARY KEY (github_login, repo)
);

CREATE TABLE IF NOT EXISTS secrets (
    id BIGSERIAL PRIMARY KEY,
    repo TEXT NOT NULL,
    name TEXT NOT NULL,
    encrypted_value TEXT NOT NULL,
    created_by TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(repo, name)
);

CREATE TABLE IF NOT EXISTS repo_policies (
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
);

CREATE TABLE IF NOT EXISTS org_policies (
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
);

CREATE TABLE IF NOT EXISTS audit_log (
    id BIGSERIAL PRIMARY KEY,
    event_type TEXT NOT NULL,
    actor TEXT NOT NULL,
    target TEXT,
    detail JSONB,
    ip_address TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_audit_log_event_type_created
    ON audit_log (event_type, created_at);
CREATE INDEX IF NOT EXISTS idx_audit_log_actor_created
    ON audit_log (actor, created_at);

CREATE TABLE IF NOT EXISTS budgets (
    id BIGSERIAL PRIMARY KEY,
    scope TEXT NOT NULL,
    scope_type TEXT NOT NULL CHECK (scope_type IN ('user', 'org')),
    monthly_limit_usd DOUBLE PRECISION NOT NULL,
    alert_threshold_pct INTEGER NOT NULL DEFAULT 80,
    created_by TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(scope, scope_type)
);

CREATE TABLE IF NOT EXISTS global_ai_config (
    id INTEGER PRIMARY KEY DEFAULT 1 CHECK (id = 1),
    ai_provider TEXT NOT NULL,
    ai_api_key_encrypted TEXT NOT NULL,
    ai_model TEXT,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_by TEXT
);

-- ============================================================
-- Seed: Users
-- ============================================================

INSERT INTO users (github_login, name, email, github_token, role)
VALUES
    ('testadmin',  'Test Admin',  'admin@test.com',  'ghp_fake_admin_token',  'admin'),
    ('testmember', 'Test Member', 'member@test.com', 'ghp_fake_member_token', 'member'),
    ('testviewer', 'Test Viewer', 'viewer@test.com', 'ghp_fake_viewer_token', 'viewer');

-- ============================================================
-- Seed: Tasks (various states)
-- ============================================================

INSERT INTO tasks (id, prompt, repo, base_branch, branch_name, agent_name, status, created_by, total_input_tokens, total_output_tokens, total_cost_usd, compute_seconds, name, created_at, updated_at)
VALUES
    ('task-pending-1', 'Add dark mode support', 'testorg/testrepo', 'main', NULL, NULL, 'pending', 'testmember', 0, 0, 0, NULL, 'Dark mode', NOW() - INTERVAL '2 hours', NOW() - INTERVAL '2 hours'),
    ('task-running-1', 'Fix login page CSS', 'testorg/testrepo', 'main', 'fix/login-css', 'claude', 'running', 'testmember', 5000, 2000, 0.12, 45, 'Login CSS fix', NOW() - INTERVAL '1 hour', NOW() - INTERVAL '5 minutes'),
    ('task-completed-1', 'Implement user settings page', 'testorg/testrepo', 'main', 'feat/user-settings', 'claude', 'completed', 'testadmin', 50000, 20000, 1.50, 300, 'User settings', NOW() - INTERVAL '1 day', NOW() - INTERVAL '12 hours'),
    ('task-failed-1', 'Migrate database to v2 schema', 'testorg/testrepo', 'main', 'chore/db-migrate', 'claude', 'failed', 'testadmin', 10000, 4000, 0.30, 120, 'DB migration', NOW() - INTERVAL '3 hours', NOW() - INTERVAL '2 hours'),
    ('task-completed-2', 'Add search functionality', 'testorg/frontend', 'main', 'feat/search', 'claude', 'completed', 'testmember', 30000, 15000, 0.95, 200, 'Search feature', NOW() - INTERVAL '2 days', NOW() - INTERVAL '1 day');

-- Task in awaiting_followup status (enables TaskChat rendering)
INSERT INTO tasks (id, prompt, repo, base_branch, branch_name, agent_name, status, created_by,
  total_input_tokens, total_output_tokens, total_cost_usd, name, preview_url, preview_slug, created_at, updated_at)
VALUES ('task-awaiting-1', 'Fix button alignment on mobile', 'testorg/testrepo', 'main',
  'fix/button-align', 'claude', 'awaiting_followup', 'testadmin',
  12000, 5000, 0.35, 'Button alignment fix', 'https://my-preview.test.example.com/fix-button', 'fix-button',
  NOW() - INTERVAL '30 minutes', NOW() - INTERVAL '5 minutes');

-- Completed task without PR (enables "Create PR" button)
INSERT INTO tasks (id, prompt, repo, base_branch, branch_name, agent_name, status, created_by,
  total_input_tokens, total_output_tokens, total_cost_usd, name, created_at, updated_at)
VALUES ('task-completed-nopr', 'Add footer component', 'testorg/testrepo', 'main',
  'feat/footer', 'claude', 'completed', 'testadmin',
  15000, 7000, 0.45, 'Footer component',
  NOW() - INTERVAL '4 hours', NOW() - INTERVAL '2 hours');

-- Task with tiny cost (exercises formatCost "<$0.01" branch)
INSERT INTO tasks (id, prompt, repo, base_branch, status, created_by,
  total_input_tokens, total_output_tokens, total_cost_usd, name, created_at, updated_at)
VALUES ('task-tiny-cost', 'Quick typo fix', 'testorg/testrepo', 'main',
  'completed', 'testmember', 500, 200, 0.005, 'Typo fix',
  NOW() - INTERVAL '1 hour', NOW() - INTERVAL '55 minutes');

-- Old task (exercises timeAgo ">30 days" branch)
INSERT INTO tasks (id, prompt, repo, base_branch, status, created_by,
  total_input_tokens, total_output_tokens, total_cost_usd, name, created_at, updated_at)
VALUES ('task-old', 'Initial project setup', 'testorg/testrepo', 'main',
  'completed', 'testadmin', 20000, 10000, 0.60, 'Project setup',
  NOW() - INTERVAL '65 days', NOW() - INTERVAL '65 days');

-- Set error_message on the failed task
UPDATE tasks SET error_message = 'Migration failed: column "legacy_data" does not exist' WHERE id = 'task-failed-1';

-- Set PR info on a completed task
UPDATE tasks SET pr_url = 'https://github.com/testorg/testrepo/pull/42', pr_number = 42 WHERE id = 'task-completed-1';

-- Set image_url on running task (exercises parseImageUrls in TaskDetail)
UPDATE tasks SET image_url = '["https://example.com/screenshot1.png","https://example.com/screenshot2.png"]' WHERE id = 'task-running-1';

-- A subtask
INSERT INTO tasks (id, prompt, repo, base_branch, branch_name, agent_name, status, created_by, parent_task_id, total_input_tokens, total_output_tokens, total_cost_usd, compute_seconds, name, created_at, updated_at)
VALUES
    ('task-subtask-1', 'Write unit tests for settings page', 'testorg/testrepo', 'main', 'feat/user-settings-tests', 'claude', 'completed', 'testadmin', 'task-completed-1', 8000, 3000, 0.25, 60, 'Settings tests', NOW() - INTERVAL '18 hours', NOW() - INTERVAL '14 hours');

-- ============================================================
-- Seed: Task logs (for the completed task)
-- ============================================================

INSERT INTO task_logs (task_id, line, timestamp)
VALUES
    ('task-completed-1', 'Starting task: Implement user settings page', NOW() - INTERVAL '1 day'),
    ('task-completed-1', 'Cloning repository testorg/testrepo...', NOW() - INTERVAL '1 day' + INTERVAL '10 seconds'),
    ('task-completed-1', 'Creating branch feat/user-settings from main', NOW() - INTERVAL '1 day' + INTERVAL '30 seconds'),
    ('task-completed-1', 'Running claude agent...', NOW() - INTERVAL '1 day' + INTERVAL '1 minute'),
    ('task-completed-1', 'Task completed successfully', NOW() - INTERVAL '12 hours');

-- ============================================================
-- Seed: Task messages (for the completed task)
-- ============================================================

INSERT INTO task_messages (task_id, sender, content, created_at)
VALUES
    ('task-completed-1', 'user', 'Please also add email notification preferences', NOW() - INTERVAL '20 hours'),
    ('task-completed-1', 'assistant', 'I have added the email notification preferences section to the settings page. Users can now toggle email notifications for task completions and failures.', NOW() - INTERVAL '19 hours');

-- Messages for the awaiting task (exercises TaskChat message rendering)
INSERT INTO task_messages (task_id, sender, content, created_at) VALUES
  ('task-awaiting-1', 'claude', 'I have fixed the button alignment. The issue was a missing flex-wrap on the container.', NOW() - INTERVAL '20 minutes'),
  ('task-awaiting-1', 'testadmin', 'Looks good, but can you also center it vertically?', NOW() - INTERVAL '15 minutes'),
  ('task-awaiting-1', 'system', 'Claude is thinking...', NOW() - INTERVAL '5 minutes');

-- ============================================================
-- Seed: Task actions (for the completed task)
-- ============================================================

INSERT INTO task_actions (task_id, action_type, tool_name, tool_input, summary, created_at)
VALUES
    ('task-completed-1', 'tool_use', 'Read', '{"file_path": "src/pages/Settings.tsx"}', 'Read the existing settings page', NOW() - INTERVAL '23 hours'),
    ('task-completed-1', 'tool_use', 'Write', '{"file_path": "src/pages/UserSettings.tsx"}', 'Created the user settings component', NOW() - INTERVAL '22 hours'),
    ('task-completed-1', 'tool_use', 'Bash', '{"command": "npm test"}', 'Ran the test suite', NOW() - INTERVAL '13 hours');

-- Policy violation action (exercises PolicyActionsSection in TaskDetail)
INSERT INTO task_actions (task_id, action_type, tool_name, summary, created_at)
VALUES ('task-completed-1', 'policy_violation', 'Bash',
  'POLICY VIOLATION: Bash — command matched blocked pattern: rm -rf /', NOW() - INTERVAL '13 hours');

-- Actions for the awaiting task (exercises ActivityTimeline rendering)
INSERT INTO task_actions (task_id, action_type, tool_name, tool_input, summary, created_at)
VALUES
    ('task-awaiting-1', 'clone', NULL, NULL, 'Cloned testorg/testrepo', NOW() - INTERVAL '25 minutes'),
    ('task-awaiting-1', 'tool_use', 'Read', '{"file_path": "src/components/Button.tsx"}', 'Reading /src/components/Button.tsx', NOW() - INTERVAL '22 minutes'),
    ('task-awaiting-1', 'tool_use', 'Read', '{"file_path": "src/styles/layout.css"}', 'Reading /src/styles/layout.css', NOW() - INTERVAL '21 minutes'),
    ('task-awaiting-1', 'file_edit', 'Write', '{"file_path": "src/components/Button.tsx"}', 'Edited src/components/Button.tsx', NOW() - INTERVAL '18 minutes'),
    ('task-awaiting-1', 'commit', NULL, NULL, 'Created commit: fix button alignment', NOW() - INTERVAL '16 minutes'),
    ('task-awaiting-1', 'push', NULL, NULL, 'Pushed to branch fix/button-align', NOW() - INTERVAL '15 minutes');

-- ============================================================
-- Seed: Task state transitions
-- ============================================================

INSERT INTO task_state_transitions (task_id, from_status, to_status, created_at)
VALUES
    ('task-completed-1', NULL, 'pending', NOW() - INTERVAL '1 day'),
    ('task-completed-1', 'pending', 'running', NOW() - INTERVAL '1 day' + INTERVAL '5 seconds'),
    ('task-completed-1', 'running', 'completed', NOW() - INTERVAL '12 hours');

-- ============================================================
-- Seed: Repo policies
-- ============================================================

INSERT INTO repo_policies (repo, protected_branches, allowed_tools, network_egress, max_cost_usd, require_approval_above_usd, created_by)
VALUES
    ('testorg/testrepo', '{main,master,production}', '["Read","Write","Bash","Grep","Glob"]', '{"allow_all": false, "allowed_hosts": ["api.github.com","registry.npmjs.org"]}', 10.0, 5.0, 'testadmin'),
    ('testorg/frontend', '{main}', NULL, NULL, 5.0, NULL, 'testadmin');

-- ============================================================
-- Seed: Org policies
-- ============================================================

INSERT INTO org_policies (org, protected_branches, allowed_tools, network_egress, max_cost_usd, require_approval_above_usd, created_by)
VALUES
    ('testorg', '{main,master}', '["Read","Write","Bash"]', '{"allow_all": true}', 50.0, 20.0, 'testadmin');

-- ============================================================
-- Seed: Secrets
-- ============================================================

INSERT INTO secrets (repo, name, encrypted_value, created_by)
VALUES
    ('testorg/testrepo', 'NPM_TOKEN', 'encrypted:fake_npm_token_value', 'testadmin'),
    ('testorg/testrepo', 'DEPLOY_KEY', 'encrypted:fake_deploy_key_value', 'testadmin'),
    ('testorg/frontend', 'API_KEY', 'encrypted:fake_api_key_value', 'testadmin');

-- ============================================================
-- Seed: Budgets
-- ============================================================

INSERT INTO budgets (scope, scope_type, monthly_limit_usd, alert_threshold_pct, created_by)
VALUES
    ('testadmin', 'user', 100.0, 80, 'testadmin'),
    ('testorg', 'org', 500.0, 90, 'testadmin');

-- ============================================================
-- Seed: Audit log entries
-- ============================================================

INSERT INTO audit_log (event_type, actor, target, detail, ip_address, created_at)
VALUES
    ('auth.login', 'testadmin', NULL, '{"role": "admin"}', '127.0.0.1', NOW() - INTERVAL '1 day'),
    ('auth.login', 'testmember', NULL, '{"role": "member"}', '127.0.0.1', NOW() - INTERVAL '1 day'),
    ('task.create', 'testadmin', 'task-completed-1', '{"repo": "testorg/testrepo", "prompt": "Implement user settings page"}', '127.0.0.1', NOW() - INTERVAL '1 day'),
    ('task.complete', 'system', 'task-completed-1', '{"cost_usd": 1.50}', NULL, NOW() - INTERVAL '12 hours'),
    ('admin.role_change', 'testadmin', 'testviewer', '{"new_role": "viewer"}', '127.0.0.1', NOW() - INTERVAL '6 hours'),
    ('admin.user_repos_changed', 'testadmin', 'testmember', '{"repos": ["testorg/testrepo","testorg/frontend"]}', '127.0.0.1', NOW() - INTERVAL '5 hours'),
    ('auth.logout', 'testmember', NULL, '{}', '127.0.0.1', NOW() - INTERVAL '4 hours'),
    -- Additional entries for pagination testing (need 27+ total for 2 pages at PER_PAGE=25)
    ('auth.login', 'testviewer', NULL, '{"role": "viewer"}', '127.0.0.1', NOW() - INTERVAL '3 days'),
    ('task.create', 'testmember', 'task-running-1', '{"repo": "testorg/testrepo", "prompt": "Fix login page CSS"}', '127.0.0.1', NOW() - INTERVAL '3 days'),
    ('task.create', 'testmember', 'task-pending-1', '{"repo": "testorg/testrepo", "prompt": "Add dark mode support"}', '127.0.0.1', NOW() - INTERVAL '2 days' + INTERVAL '1 hour'),
    ('task.create', 'testadmin', 'task-failed-1', '{"repo": "testorg/testrepo", "prompt": "Migrate database"}', '127.0.0.1', NOW() - INTERVAL '2 days' + INTERVAL '2 hours'),
    ('task.create', 'testmember', 'task-completed-2', '{"repo": "testorg/frontend", "prompt": "Add search"}', '127.0.0.1', NOW() - INTERVAL '2 days' + INTERVAL '4 hours'),
    ('task.complete', 'system', 'task-completed-2', '{"cost_usd": 0.95}', NULL, NOW() - INTERVAL '1 day' + INTERVAL '1 hour'),
    ('admin.secret_created', 'testadmin', 'testorg/testrepo:NPM_TOKEN', '{}', '127.0.0.1', NOW() - INTERVAL '7 days'),
    ('admin.secret_created', 'testadmin', 'testorg/testrepo:DEPLOY_KEY', '{}', '127.0.0.1', NOW() - INTERVAL '7 days'),
    ('admin.secret_created', 'testadmin', 'testorg/frontend:API_KEY', '{}', '127.0.0.1', NOW() - INTERVAL '7 days'),
    ('admin.policy_created', 'testadmin', 'testorg/testrepo', '{"max_cost_usd": 10.0}', '127.0.0.1', NOW() - INTERVAL '7 days'),
    ('admin.policy_created', 'testadmin', 'testorg/frontend', '{"max_cost_usd": 5.0}', '127.0.0.1', NOW() - INTERVAL '7 days'),
    ('admin.org_policy_created', 'testadmin', 'testorg', '{"max_cost_usd": 50.0}', '127.0.0.1', NOW() - INTERVAL '7 days'),
    ('admin.budget_created', 'testadmin', 'testadmin', '{"monthly_limit_usd": 100.0}', '127.0.0.1', NOW() - INTERVAL '7 days'),
    ('admin.budget_created', 'testadmin', 'testorg', '{"monthly_limit_usd": 500.0}', '127.0.0.1', NOW() - INTERVAL '7 days'),
    ('auth.login', 'testadmin', NULL, '{"role": "admin"}', '192.168.1.1', NOW() - INTERVAL '2 days'),
    ('auth.login', 'testmember', NULL, '{"role": "member"}', '192.168.1.2', NOW() - INTERVAL '2 days'),
    ('auth.login', 'testviewer', NULL, '{"role": "viewer"}', '192.168.1.3', NOW() - INTERVAL '2 days'),
    ('auth.logout', 'testadmin', NULL, '{}', '127.0.0.1', NOW() - INTERVAL '2 days' + INTERVAL '8 hours'),
    ('auth.logout', 'testviewer', NULL, '{}', '127.0.0.1', NOW() - INTERVAL '2 days' + INTERVAL '6 hours'),
    ('task.fail', 'system', 'task-failed-1', '{"error": "Migration failed"}', NULL, NOW() - INTERVAL '2 days' + INTERVAL '3 hours');

-- ============================================================
-- Seed: User repo permissions
-- ============================================================

INSERT INTO user_repo_permissions (github_login, repo)
VALUES
    ('testmember', 'testorg/testrepo'),
    ('testmember', 'testorg/frontend'),
    ('testviewer', 'testorg/testrepo');

-- ============================================================
-- Intake: Add columns to tasks table
-- ============================================================

ALTER TABLE tasks ADD COLUMN IF NOT EXISTS intake_issue_id BIGINT;
ALTER TABLE tasks ADD COLUMN IF NOT EXISTS source_type TEXT DEFAULT 'manual';

-- ============================================================
-- Intake: Create tables
-- ============================================================

CREATE TABLE IF NOT EXISTS intake_sources (
    id BIGSERIAL PRIMARY KEY,
    name TEXT NOT NULL,
    provider TEXT NOT NULL,
    enabled BOOLEAN NOT NULL DEFAULT true,
    config JSONB NOT NULL DEFAULT '{}',
    api_token_encrypted TEXT NOT NULL,
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
);

CREATE TABLE IF NOT EXISTS intake_issues (
    id BIGSERIAL PRIMARY KEY,
    source_id BIGINT NOT NULL REFERENCES intake_sources(id) ON DELETE CASCADE,
    external_id TEXT NOT NULL,
    external_url TEXT,
    external_title TEXT NOT NULL,
    external_body TEXT,
    external_labels TEXT[] NOT NULL DEFAULT '{}',
    external_updated_at TIMESTAMPTZ,
    task_id TEXT REFERENCES tasks(id),
    status TEXT NOT NULL DEFAULT 'pending',
    error_message TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(source_id, external_id)
);

CREATE TABLE IF NOT EXISTS intake_poll_log (
    id BIGSERIAL PRIMARY KEY,
    source_id BIGINT NOT NULL REFERENCES intake_sources(id) ON DELETE CASCADE,
    polled_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    issues_found INTEGER NOT NULL DEFAULT 0,
    issues_created INTEGER NOT NULL DEFAULT 0,
    issues_skipped INTEGER NOT NULL DEFAULT 0,
    error_message TEXT,
    duration_ms INTEGER
);

-- ============================================================
-- Seed: Intake sources
-- ============================================================

INSERT INTO intake_sources (name, provider, enabled, api_token_encrypted, target_repo, run_as_user, poll_interval_secs, created_by)
VALUES
    ('GitHub Bugs', 'github', true, 'encrypted:fake_gh_token', 'testorg/testrepo', 'testadmin', 120, 'testadmin'),
    ('Linear Features', 'linear', false, 'encrypted:fake_linear_token', 'testorg/frontend', 'testmember', 300, 'testadmin');

-- ============================================================
-- Seed: Intake issues (8 rows covering all 6 statuses)
-- ============================================================

INSERT INTO intake_issues (source_id, external_id, external_url, external_title, external_body, external_labels, status, task_id, error_message, created_at, updated_at)
VALUES
    (1, 'GH-101', 'https://github.com/testorg/testrepo/issues/101', 'Fix null pointer in auth module', 'Auth module throws NPE when token is expired.', '{bug,auth}', 'backlog', NULL, NULL, NOW() - INTERVAL '2 days', NOW() - INTERVAL '2 days'),
    (1, 'GH-102', 'https://github.com/testorg/testrepo/issues/102', 'Login page crashes on Safari', 'Safari 17 shows blank screen on login.', '{bug,frontend}', 'pending', NULL, NULL, NOW() - INTERVAL '1 day', NOW() - INTERVAL '1 day'),
    (1, 'GH-103', 'https://github.com/testorg/testrepo/issues/103', 'Add rate limiting to API', 'Need rate limiting on public endpoints.', '{enhancement}', 'task_created', 'task-running-1', NULL, NOW() - INTERVAL '1 day', NOW() - INTERVAL '12 hours'),
    (1, 'GH-104', 'https://github.com/testorg/testrepo/issues/104', 'Review security headers', 'Audit response headers for OWASP compliance.', '{security,review}', 'review', 'task-completed-1', NULL, NOW() - INTERVAL '3 days', NOW() - INTERVAL '6 hours'),
    (1, 'GH-105', 'https://github.com/testorg/testrepo/issues/105', 'Upgrade dependencies to latest', 'Bump all deps to latest patch versions.', '{dependencies}', 'done', 'task-completed-2', NULL, NOW() - INTERVAL '4 days', NOW() - INTERVAL '1 day'),
    (1, 'GH-106', 'https://github.com/testorg/testrepo/issues/106', 'Fix flaky CI test', 'test_auth_flow fails intermittently.', '{bug,ci}', 'failed', 'task-failed-1', 'Agent timed out after 300s', NOW() - INTERVAL '2 days', NOW() - INTERVAL '3 hours'),
    (2, 'LIN-201', 'https://linear.app/testorg/issue/LIN-201', 'Add dark mode toggle', 'Users want dark mode support.', '{feature,ux}', 'backlog', NULL, NULL, NOW() - INTERVAL '5 days', NOW() - INTERVAL '5 days'),
    (2, 'LIN-202', 'https://linear.app/testorg/issue/LIN-202', 'Implement CSV export', 'Export table data as CSV.', '{feature}', 'pending', NULL, NULL, NOW() - INTERVAL '3 days', NOW() - INTERVAL '2 days');

-- ============================================================
-- Seed: Intake poll log
-- ============================================================

INSERT INTO intake_poll_log (source_id, polled_at, issues_found, issues_created, issues_skipped, error_message, duration_ms)
VALUES
    (1, NOW() - INTERVAL '6 hours', 3, 2, 1, NULL, 1200),
    (1, NOW() - INTERVAL '4 hours', 1, 1, 0, NULL, 800),
    (1, NOW() - INTERVAL '2 hours', 2, 0, 2, NULL, 950),
    (1, NOW() - INTERVAL '30 minutes', 0, 0, 0, 'Rate limit exceeded, partial results', 3500),
    (2, NOW() - INTERVAL '5 hours', 2, 2, 0, NULL, 1500),
    (2, NOW() - INTERVAL '1 hour', 1, 0, 1, NULL, 600);

-- ============================================================
-- Link tasks to intake issues
-- ============================================================

-- ============================================================
-- Seed: Sync-test tasks and intake issues (dedicated for intake-sync.spec.ts)
-- ============================================================

INSERT INTO tasks (id, prompt, repo, base_branch, agent_name, status, created_by, name, created_at, updated_at)
VALUES
    ('task-sync-1', 'Sync test task 1', 'testorg/testrepo', 'main', 'claude', 'running_claude', 'testadmin', 'Sync task 1', NOW() - INTERVAL '1 hour', NOW() - INTERVAL '30 minutes'),
    ('task-sync-2', 'Sync test task 2', 'testorg/testrepo', 'main', 'claude', 'running_claude', 'testadmin', 'Sync task 2', NOW() - INTERVAL '1 hour', NOW() - INTERVAL '30 minutes');

INSERT INTO intake_issues (source_id, external_id, external_url, external_title, status, task_id, created_at, updated_at)
VALUES
    (1, 'SYNC-1', 'https://github.com/testorg/testrepo/issues/901', 'Sync test issue 1', 'task_created', 'task-sync-1', NOW() - INTERVAL '1 hour', NOW() - INTERVAL '30 minutes'),
    (1, 'SYNC-2', 'https://github.com/testorg/testrepo/issues/902', 'Sync test issue 2', 'review',       'task-sync-2', NOW() - INTERVAL '1 hour', NOW() - INTERVAL '30 minutes'),
    (1, 'SYNC-3', 'https://github.com/testorg/testrepo/issues/903', 'Sync test issue 3', 'backlog',      NULL,          NOW() - INTERVAL '1 hour', NOW() - INTERVAL '30 minutes');

-- ============================================================
-- Link tasks to intake issues
-- ============================================================

UPDATE tasks SET source_type = 'intake_github', intake_issue_id = 3 WHERE id = 'task-running-1';
UPDATE tasks SET source_type = 'intake_github', intake_issue_id = 4 WHERE id = 'task-completed-1';
UPDATE tasks SET source_type = 'intake_github', intake_issue_id = 5 WHERE id = 'task-completed-2';
UPDATE tasks SET source_type = 'intake_github', intake_issue_id = 6 WHERE id = 'task-failed-1';
