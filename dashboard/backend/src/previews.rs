use axum::extract::{Path, State};
use axum::Json;

use crate::auth::{self, AuthUser, MemberUser};
use crate::error::AppError;
use crate::models::{CreatePreviewRequest, Preview};
use crate::shell;
use crate::AppState;

pub async fn list_previews(
    _user: AuthUser,
    State(state): State<AppState>,
) -> Result<Json<Vec<Preview>>, AppError> {
    let previews = shell::list_previews(&state.config).await?;
    Ok(Json(previews))
}

pub async fn create_preview(
    user: MemberUser,
    State(state): State<AppState>,
    Json(req): Json<CreatePreviewRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    if !state.config.allowed_repos.is_empty() && !state.config.allowed_repos.contains(&req.repo) {
        return Err(AppError::BadRequest(format!(
            "Repo '{}' is not in the allowed list",
            req.repo
        )));
    }

    authorize_repo(&state, &user, &req.repo).await?;

    let output = shell::create_preview(&state.config, &req.repo, &req.branch, req.slug.as_deref())
        .await
        .map_err(|e| classify_create_error(e, &req.repo, req.slug.as_deref()))?;

    // Audit: preview.created
    crate::audit::log_event(
        &state.db,
        "preview.created",
        &user.0.sub,
        req.slug.as_deref(),
        serde_json::json!({ "repo": &req.repo, "branch": &req.branch }),
        None,
    )
    .await;

    Ok(Json(serde_json::json!({
        "message": "Preview creation started",
        "output": output.trim(),
    })))
}

pub async fn destroy_preview(
    _user: MemberUser,
    State(state): State<AppState>,
    Path(slug): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let output = shell::destroy_preview(&state.config, &slug).await?;

    // Audit: preview.destroyed
    crate::audit::log_event(
        &state.db,
        "preview.destroyed",
        &_user.0.sub,
        Some(&slug),
        serde_json::json!({}),
        None,
    )
    .await;

    Ok(Json(serde_json::json!({
        "message": "Preview destroyed",
        "output": output.trim(),
    })))
}

pub async fn update_preview(
    user: MemberUser,
    State(state): State<AppState>,
    Path(slug): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let repo = shell::preview_repo(&slug)
        .ok_or_else(|| AppError::NotFound(format!("Preview '{slug}' not found")))?;
    authorize_repo(&state, &user, &repo).await?;
    let output = shell::update_preview(&state.config, &slug).await?;

    // Audit: preview.updated
    crate::audit::log_event(
        &state.db,
        "preview.updated",
        &user.0.sub,
        Some(&slug),
        serde_json::json!({}),
        None,
    )
    .await;

    Ok(Json(serde_json::json!({
        "message": "Preview update triggered",
        "output": output.trim(),
    })))
}

/// Checks that `user` may preview `repo`: Tekton's own access rules, then GitHub's. `preview`
/// reads the repo with the GitHub App token, which can read every repo the App is installed
/// on, so the user's own token is what proves they can read this one. It's only used for this
/// check and never reaches `preview`.
async fn authorize_repo(state: &AppState, user: &MemberUser, repo: &str) -> Result<(), AppError> {
    auth::check_repo_permission(
        &state.db,
        &user.0.sub,
        repo,
        &user.0.role,
        &state.config.github_org,
    )
    .await?;
    let token = auth::github_token_for(state, &user.0.sub).await?;
    require_github_read(&token, repo).await
}

/// Whether `repo` is a plain `owner/name`, safe to put in an API path.
fn valid_repo_name(repo: &str) -> bool {
    let part_ok = |s: &str| {
        !s.is_empty()
            && !s.starts_with('.')
            && s.chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
    };
    matches!(repo.split_once('/'), Some((owner, name)) if part_ok(owner) && part_ok(name))
}

/// Fails unless GitHub says `token`'s user can read `repo`.
async fn require_github_read(token: &str, repo: &str) -> Result<(), AppError> {
    if !valid_repo_name(repo) {
        return Err(AppError::BadRequest(format!("Invalid repository '{repo}'")));
    }
    let resp = reqwest::Client::new()
        .get(format!("https://api.github.com/repos/{repo}"))
        .timeout(std::time::Duration::from_secs(8))
        .header("User-Agent", "dashboard")
        .header("Accept", "application/vnd.github+json")
        .bearer_auth(token)
        .send()
        .await?;
    let status = resp.status();
    if status == reqwest::StatusCode::UNAUTHORIZED {
        return Err(AppError::ReauthRequired(
            "GitHub rejected your saved token (expired or revoked); sign in again".into(),
        ));
    }
    if status == reqwest::StatusCode::NOT_FOUND || status == reqwest::StatusCode::FORBIDDEN {
        return Err(AppError::Forbidden(format!(
            "Your GitHub account can't read '{repo}'"
        )));
    }
    if !status.is_success() {
        return Err(AppError::Internal(format!(
            "Checking your GitHub access to '{repo}' failed: GitHub returned {status}"
        )));
    }
    let body: serde_json::Value = resp.json().await?;
    if body["permissions"]["pull"].as_bool() == Some(true) {
        Ok(())
    } else {
        Err(AppError::Forbidden(format!(
            "Your GitHub account can't read '{repo}'"
        )))
    }
}

/// Inspect the raw shell error from `create_preview` and return a user-friendly
/// `AppError::UserError` when the failure matches a known pattern.
fn classify_create_error(err: AppError, repo: &str, slug: Option<&str>) -> AppError {
    let msg = match &err {
        AppError::Internal(msg) => msg.as_str(),
        _ => return err,
    };

    let lower = msg.to_lowercase();

    // Repository not accessible
    if lower.contains("repository") && lower.contains("not found")
        || lower.contains("could not read from remote repository")
        || lower.contains("fatal: remote error")
        || lower.contains("authentication failed")
        || lower.contains("could not resolve host")
    {
        tracing::warn!("Preview creation failed (repo_not_accessible): {msg}");
        return AppError::UserError {
            code: "repo_not_accessible",
            message: format!(
                "Could not access repository '{repo}'. Check that the name is correct and that the deploy token has access."
            ),
        };
    }

    // Slug / container conflict
    if lower.contains("already exists")
        || lower.contains("name conflict")
        || lower.contains("is already in use")
    {
        let slug_display = slug.unwrap_or("(auto)");
        tracing::warn!("Preview creation failed (slug_conflict): {msg}");
        return AppError::UserError {
            code: "slug_conflict",
            message: format!(
                "A preview with slug '{slug_display}' already exists. Choose a different slug or destroy the existing preview first."
            ),
        };
    }

    // Invalid slug characters
    if lower.contains("invalid slug")
        || lower.contains("invalid container name")
        || lower.contains("invalid character")
    {
        tracing::warn!("Preview creation failed (invalid_slug): {msg}");
        return AppError::UserError {
            code: "invalid_slug",
            message: "The slug contains invalid characters. Use only lowercase letters, numbers, and hyphens.".to_string(),
        };
    }

    // Slug too long
    if lower.contains("is too long") {
        tracing::warn!("Preview creation failed (slug_too_long): {msg}");
        return AppError::UserError {
            code: "slug_too_long",
            message: "The slug is too long. It must be 11 characters or fewer.".to_string(),
        };
    }

    // Branch not found
    if lower.contains("did not match any")
        || (lower.contains("pathspec") && lower.contains("did not match"))
        || lower.contains("remote branch") && lower.contains("not found")
    {
        tracing::warn!("Preview creation failed (branch_not_found): {msg}");
        return AppError::UserError {
            code: "branch_not_found",
            message: format!(
                "Could not find the specified branch in '{repo}'. Check that the branch name is correct."
            ),
        };
    }

    // Unrecognised — keep original Internal error
    err
}

#[cfg(test)]
mod repo_name_tests {
    use super::valid_repo_name;

    #[test]
    fn repo_names() {
        for ok in ["o/r", "my-org/my.repo", "a_b/c-d"] {
            assert!(valid_repo_name(ok), "{ok}");
        }
        for bad in [
            "", "o", "o/", "/r", "o/r/x", "../r", "o/..", "o/.git", "o/r?x", "o/r#x", "o r/x",
        ] {
            assert!(!valid_repo_name(bad), "{bad:?}");
        }
    }
}
