use axum::extract::{Path, State};
use axum::Json;
use sqlx::PgPool;

use crate::auth::AdminUser;
use crate::error::AppError;
use crate::models::{
    CreateOrgPolicyRequest, CreateRepoPolicyRequest, OrgPolicy, RepoPolicy,
    UpdateOrgPolicyRequest, UpdateRepoPolicyRequest,
};

/// GET /api/admin/policies
pub async fn list_policies(
    _admin: AdminUser,
    State(state): State<crate::AppState>,
) -> Result<Json<Vec<RepoPolicy>>, AppError> {
    let policies = sqlx::query_as::<_, RepoPolicy>(
        "SELECT id, repo, protected_branches, allowed_tools, network_egress, \
         max_cost_usd, require_approval_above_usd, created_by, \
         TO_CHAR(created_at, 'YYYY-MM-DD HH24:MI:SS') as created_at, \
         TO_CHAR(updated_at, 'YYYY-MM-DD HH24:MI:SS') as updated_at \
         FROM repo_policies ORDER BY repo",
    )
    .fetch_all(&state.db)
    .await?;
    Ok(Json(policies))
}

/// POST /api/admin/policies
pub async fn create_policy(
    admin: AdminUser,
    State(state): State<crate::AppState>,
    Json(req): Json<CreateRepoPolicyRequest>,
) -> Result<Json<RepoPolicy>, AppError> {
    let protected_branches = req
        .protected_branches
        .unwrap_or_else(|| vec!["main".into(), "master".into()]);

    sqlx::query(
        "INSERT INTO repo_policies (repo, protected_branches, allowed_tools, network_egress, \
         max_cost_usd, require_approval_above_usd, created_by) \
         VALUES ($1, $2, $3, $4, $5, $6, $7) \
         ON CONFLICT (repo) DO UPDATE SET \
           protected_branches = EXCLUDED.protected_branches, \
           allowed_tools = EXCLUDED.allowed_tools, \
           network_egress = EXCLUDED.network_egress, \
           max_cost_usd = EXCLUDED.max_cost_usd, \
           require_approval_above_usd = EXCLUDED.require_approval_above_usd, \
           created_by = EXCLUDED.created_by, \
           updated_at = NOW()",
    )
    .bind(&req.repo)
    .bind(&protected_branches)
    .bind(&req.allowed_tools)
    .bind(&req.network_egress)
    .bind(req.max_cost_usd)
    .bind(req.require_approval_above_usd)
    .bind(&admin.0.sub)
    .execute(&state.db)
    .await?;

    let policy = sqlx::query_as::<_, RepoPolicy>(
        "SELECT id, repo, protected_branches, allowed_tools, network_egress, \
         max_cost_usd, require_approval_above_usd, created_by, \
         TO_CHAR(created_at, 'YYYY-MM-DD HH24:MI:SS') as created_at, \
         TO_CHAR(updated_at, 'YYYY-MM-DD HH24:MI:SS') as updated_at \
         FROM repo_policies WHERE repo = $1",
    )
    .bind(&req.repo)
    .fetch_one(&state.db)
    .await?;

    Ok(Json(policy))
}

/// PUT /api/admin/policies/{id}
pub async fn update_policy(
    _admin: AdminUser,
    State(state): State<crate::AppState>,
    Path(id): Path<i64>,
    Json(req): Json<UpdateRepoPolicyRequest>,
) -> Result<Json<RepoPolicy>, AppError> {
    // Build dynamic UPDATE query for only the provided fields
    let mut sets = Vec::new();
    let mut param_idx = 2u32; // $1 is id

    if req.protected_branches.is_some() {
        sets.push(format!("protected_branches = ${param_idx}"));
        param_idx += 1;
    }
    if req.allowed_tools.is_some() {
        sets.push(format!("allowed_tools = ${param_idx}"));
        param_idx += 1;
    }
    if req.network_egress.is_some() {
        sets.push(format!("network_egress = ${param_idx}"));
        param_idx += 1;
    }
    if req.max_cost_usd.is_some() {
        sets.push(format!("max_cost_usd = ${param_idx}"));
        param_idx += 1;
    }
    if req.require_approval_above_usd.is_some() {
        sets.push(format!("require_approval_above_usd = ${param_idx}"));
        param_idx += 1;
    }

    if sets.is_empty() {
        return Err(AppError::BadRequest("No fields to update".into()));
    }

    sets.push("updated_at = NOW()".into());
    let _ = param_idx;

    let sql = format!(
        "UPDATE repo_policies SET {} WHERE id = $1 \
         RETURNING id, repo, protected_branches, allowed_tools, network_egress, \
         max_cost_usd, require_approval_above_usd, created_by, \
         TO_CHAR(created_at, 'YYYY-MM-DD HH24:MI:SS') as created_at, \
         TO_CHAR(updated_at, 'YYYY-MM-DD HH24:MI:SS') as updated_at",
        sets.join(", ")
    );

    let mut query = sqlx::query_as::<_, RepoPolicy>(&sql).bind(id);

    if let Some(ref branches) = req.protected_branches {
        query = query.bind(branches);
    }
    if let Some(ref tools) = req.allowed_tools {
        query = query.bind(tools);
    }
    if let Some(ref egress) = req.network_egress {
        query = query.bind(egress);
    }
    if let Some(cost) = req.max_cost_usd {
        query = query.bind(cost);
    }
    if let Some(approval) = req.require_approval_above_usd {
        query = query.bind(approval);
    }

    let policy = query.fetch_optional(&state.db).await?;

    match policy {
        Some(p) => Ok(Json(p)),
        None => Err(AppError::NotFound("Policy not found".into())),
    }
}

/// DELETE /api/admin/policies/{id}
pub async fn delete_policy(
    _admin: AdminUser,
    State(state): State<crate::AppState>,
    Path(id): Path<i64>,
) -> Result<Json<serde_json::Value>, AppError> {
    let result = sqlx::query("DELETE FROM repo_policies WHERE id = $1")
        .bind(id)
        .execute(&state.db)
        .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound("Policy not found".into()));
    }

    Ok(Json(serde_json::json!({ "deleted": true })))
}

// ── Org Policy CRUD ──

const ORG_POLICY_COLUMNS: &str = "id, org, protected_branches, allowed_tools, network_egress, \
     max_cost_usd, require_approval_above_usd, created_by, \
     TO_CHAR(created_at, 'YYYY-MM-DD HH24:MI:SS') as created_at, \
     TO_CHAR(updated_at, 'YYYY-MM-DD HH24:MI:SS') as updated_at";

/// GET /api/admin/org-policies
pub async fn list_org_policies(
    _admin: AdminUser,
    State(state): State<crate::AppState>,
) -> Result<Json<Vec<OrgPolicy>>, AppError> {
    let policies = sqlx::query_as::<_, OrgPolicy>(&format!(
        "SELECT {ORG_POLICY_COLUMNS} FROM org_policies ORDER BY org"
    ))
    .fetch_all(&state.db)
    .await?;
    Ok(Json(policies))
}

/// POST /api/admin/org-policies
pub async fn create_org_policy(
    admin: AdminUser,
    State(state): State<crate::AppState>,
    Json(req): Json<CreateOrgPolicyRequest>,
) -> Result<Json<OrgPolicy>, AppError> {
    let protected_branches = req
        .protected_branches
        .unwrap_or_else(|| vec!["main".into(), "master".into()]);

    sqlx::query(
        "INSERT INTO org_policies (org, protected_branches, allowed_tools, network_egress, \
         max_cost_usd, require_approval_above_usd, created_by) \
         VALUES ($1, $2, $3, $4, $5, $6, $7) \
         ON CONFLICT (org) DO UPDATE SET \
           protected_branches = EXCLUDED.protected_branches, \
           allowed_tools = EXCLUDED.allowed_tools, \
           network_egress = EXCLUDED.network_egress, \
           max_cost_usd = EXCLUDED.max_cost_usd, \
           require_approval_above_usd = EXCLUDED.require_approval_above_usd, \
           created_by = EXCLUDED.created_by, \
           updated_at = NOW()",
    )
    .bind(&req.org)
    .bind(&protected_branches)
    .bind(&req.allowed_tools)
    .bind(&req.network_egress)
    .bind(req.max_cost_usd)
    .bind(req.require_approval_above_usd)
    .bind(&admin.0.sub)
    .execute(&state.db)
    .await?;

    let policy = sqlx::query_as::<_, OrgPolicy>(&format!(
        "SELECT {ORG_POLICY_COLUMNS} FROM org_policies WHERE org = $1"
    ))
    .bind(&req.org)
    .fetch_one(&state.db)
    .await?;

    Ok(Json(policy))
}

/// PUT /api/admin/org-policies/{id}
pub async fn update_org_policy(
    _admin: AdminUser,
    State(state): State<crate::AppState>,
    Path(id): Path<i64>,
    Json(req): Json<UpdateOrgPolicyRequest>,
) -> Result<Json<OrgPolicy>, AppError> {
    let mut sets = Vec::new();
    let mut param_idx = 2u32;

    if req.protected_branches.is_some() {
        sets.push(format!("protected_branches = ${param_idx}"));
        param_idx += 1;
    }
    if req.allowed_tools.is_some() {
        sets.push(format!("allowed_tools = ${param_idx}"));
        param_idx += 1;
    }
    if req.network_egress.is_some() {
        sets.push(format!("network_egress = ${param_idx}"));
        param_idx += 1;
    }
    if req.max_cost_usd.is_some() {
        sets.push(format!("max_cost_usd = ${param_idx}"));
        param_idx += 1;
    }
    if req.require_approval_above_usd.is_some() {
        sets.push(format!("require_approval_above_usd = ${param_idx}"));
        param_idx += 1;
    }

    if sets.is_empty() {
        return Err(AppError::BadRequest("No fields to update".into()));
    }

    sets.push("updated_at = NOW()".into());
    let _ = param_idx;

    let sql = format!(
        "UPDATE org_policies SET {} WHERE id = $1 \
         RETURNING {ORG_POLICY_COLUMNS}",
        sets.join(", ")
    );

    let mut query = sqlx::query_as::<_, OrgPolicy>(&sql).bind(id);

    if let Some(ref branches) = req.protected_branches {
        query = query.bind(branches);
    }
    if let Some(ref tools) = req.allowed_tools {
        query = query.bind(tools);
    }
    if let Some(ref egress) = req.network_egress {
        query = query.bind(egress);
    }
    if let Some(cost) = req.max_cost_usd {
        query = query.bind(cost);
    }
    if let Some(approval) = req.require_approval_above_usd {
        query = query.bind(approval);
    }

    let policy = query.fetch_optional(&state.db).await?;

    match policy {
        Some(p) => Ok(Json(p)),
        None => Err(AppError::NotFound("Org policy not found".into())),
    }
}

/// DELETE /api/admin/org-policies/{id}
pub async fn delete_org_policy(
    _admin: AdminUser,
    State(state): State<crate::AppState>,
    Path(id): Path<i64>,
) -> Result<Json<serde_json::Value>, AppError> {
    let result = sqlx::query("DELETE FROM org_policies WHERE id = $1")
        .bind(id)
        .execute(&state.db)
        .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound("Org policy not found".into()));
    }

    Ok(Json(serde_json::json!({ "deleted": true })))
}

// ── Helpers for pipeline use ──

/// Load the policy for a specific repo, if one exists.
pub async fn load_policy_for_repo(
    db: &PgPool,
    repo: &str,
) -> Result<Option<RepoPolicy>, AppError> {
    let policy = sqlx::query_as::<_, RepoPolicy>(
        "SELECT id, repo, protected_branches, allowed_tools, network_egress, \
         max_cost_usd, require_approval_above_usd, created_by, \
         TO_CHAR(created_at, 'YYYY-MM-DD HH24:MI:SS') as created_at, \
         TO_CHAR(updated_at, 'YYYY-MM-DD HH24:MI:SS') as updated_at \
         FROM repo_policies WHERE repo = $1",
    )
    .bind(repo)
    .fetch_optional(db)
    .await?;
    Ok(policy)
}

/// Load the org-level policy by extracting the org (owner) from "owner/repo".
pub async fn load_policy_for_org(
    db: &PgPool,
    repo: &str,
) -> Result<Option<OrgPolicy>, AppError> {
    let org = repo.split('/').next().unwrap_or(repo);
    let policy = sqlx::query_as::<_, OrgPolicy>(&format!(
        "SELECT {ORG_POLICY_COLUMNS} FROM org_policies WHERE org = $1"
    ))
    .bind(org)
    .fetch_optional(db)
    .await?;
    Ok(policy)
}

/// Load the effective (merged) policy for a repo.
///
/// Inheritance: start with the org policy as base, then overlay repo policy
/// fields on top (repo policy wins for any field that is set/non-null).
/// The result is returned as a `RepoPolicy` so callers don't need to change.
pub async fn load_effective_policy(
    db: &PgPool,
    repo: &str,
) -> Result<Option<RepoPolicy>, AppError> {
    let org_policy = load_policy_for_org(db, repo).await?;
    let repo_policy = load_policy_for_repo(db, repo).await?;

    match (org_policy, repo_policy) {
        (None, None) => Ok(None),
        (None, Some(rp)) => Ok(Some(rp)),
        (Some(op), None) => Ok(Some(RepoPolicy {
            id: op.id,
            repo: repo.to_string(),
            protected_branches: op.protected_branches,
            allowed_tools: op.allowed_tools,
            network_egress: op.network_egress,
            max_cost_usd: op.max_cost_usd,
            require_approval_above_usd: op.require_approval_above_usd,
            created_by: op.created_by,
            created_at: op.created_at,
            updated_at: op.updated_at,
        })),
        (Some(op), Some(rp)) => {
            // Merge: repo policy fields win when set; fall back to org policy
            Ok(Some(RepoPolicy {
                id: rp.id,
                repo: rp.repo,
                protected_branches: if rp.protected_branches.is_empty() {
                    op.protected_branches
                } else {
                    rp.protected_branches
                },
                allowed_tools: rp.allowed_tools.or(op.allowed_tools),
                network_egress: rp.network_egress.or(op.network_egress),
                max_cost_usd: rp.max_cost_usd.or(op.max_cost_usd),
                require_approval_above_usd: rp
                    .require_approval_above_usd
                    .or(op.require_approval_above_usd),
                created_by: rp.created_by,
                created_at: rp.created_at,
                updated_at: rp.updated_at,
            }))
        }
    }
}
