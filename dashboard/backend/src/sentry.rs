use std::sync::{Arc, LazyLock};

use regex::Regex;
use serde::Deserialize;
use sqlx::PgPool;

use crate::config::Config;
use crate::error::AppError;
use crate::models::SentrySource;

pub const SOURCE_COLUMNS: &str = "id, name, enabled, sentry_org, sentry_projects, environments, \
     auth_token_encrypted <> '' as has_token, target_repo, labels, run_as_user, \
     TO_CHAR(last_polled_at, 'YYYY-MM-DD HH24:MI:SS') as last_polled_at, last_error, \
     created_by, \
     TO_CHAR(created_at, 'YYYY-MM-DD HH24:MI:SS') as created_at, \
     TO_CHAR(updated_at, 'YYYY-MM-DD HH24:MI:SS') as updated_at";

/// Columns the poller needs on top of `SOURCE_COLUMNS`. `cutoff` is the source's
/// creation time in UTC: only Sentry issues first seen after it get filed, so
/// enabling a source doesn't dump a project's whole unresolved history on GitHub.
pub const SECRET_COLUMNS: &str = "auth_token_encrypted, \
     TO_CHAR(created_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS') as cutoff";

const SENTRY_API: &str = "https://sentry.io/api/0";
const POLL_INTERVAL_SECS: i32 = 300;
// Anything over the cap is filed on the next poll
const MAX_ISSUES_PER_POLL: usize = 10;
// GitHub rejects issue bodies over 65536 characters
const MAX_BODY_CHARS: usize = 60_000;
const MAX_TITLE_CHARS: usize = 250;
const MAX_FRAMES: usize = 15;
const MAX_CRUMBS: usize = 10;
const EVENT_TAGS: &[&str] = &[
    "environment",
    "release",
    "browser",
    "os",
    "runtime",
    "handled",
    "mechanism",
    "level",
    "app",
];

#[derive(sqlx::FromRow)]
pub struct SentrySourceWithSecret {
    #[sqlx(flatten)]
    pub source: SentrySource,
    pub auth_token_encrypted: String,
    pub cutoff: String,
}

// ── Sentry API ──

#[derive(Debug, Clone, Deserialize)]
pub struct SentryProject {
    pub slug: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SentryApiIssue {
    pub id: String,
    pub short_id: String,
    pub title: String,
    pub permalink: String,
    pub project: SentryProject,
    #[serde(default)]
    pub level: Option<String>,
    // Sentry sends `count` as a string and `userCount` as a number
    #[serde(default)]
    pub count: Option<serde_json::Value>,
    #[serde(default)]
    pub user_count: Option<serde_json::Value>,
    #[serde(default)]
    pub first_seen: Option<String>,
    #[serde(default)]
    pub last_seen: Option<String>,
}

async fn sentry_get(
    path: &str,
    token: &str,
    query: &[(&str, &str)],
) -> Result<serde_json::Value, AppError> {
    let resp = reqwest::Client::new()
        .get(format!("{SENTRY_API}/{path}"))
        .query(query)
        .header("Authorization", format!("Bearer {token}"))
        .header("User-Agent", "tekton-sentry")
        .send()
        .await
        .map_err(|e| AppError::Internal(format!("Sentry API request failed: {e}")))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(AppError::Internal(format!(
            "Sentry API returned {status}: {body}"
        )));
    }
    resp.json()
        .await
        .map_err(|e| AppError::Internal(format!("Failed to parse Sentry response: {e}")))
}

/// Unresolved issues in the source's projects and environments first seen
/// after `cutoff`, newest first.
pub async fn fetch_sentry_issues(
    source: &SentrySource,
    token: &str,
    cutoff: &str,
) -> Result<Vec<SentryApiIssue>, AppError> {
    let search = format!("is:unresolved firstSeen:>{cutoff}");
    let mut query = vec![
        ("project", "-1"),
        ("query", search.as_str()),
        ("sort", "new"),
        ("limit", "100"),
    ];
    for env in &source.environments {
        query.push(("environment", env));
    }

    let value = sentry_get(
        &format!("organizations/{}/issues/", source.sentry_org),
        token,
        &query,
    )
    .await?;
    let issues: Vec<SentryApiIssue> = serde_json::from_value(value)
        .map_err(|e| AppError::Internal(format!("Failed to parse Sentry issues: {e}")))?;

    // The org endpoint only filters by numeric project ID, so match slugs here
    Ok(issues
        .into_iter()
        .filter(|issue| {
            source.sentry_projects.is_empty()
                || source.sentry_projects.contains(&issue.project.slug)
        })
        .collect())
}

/// Sentry issues that don't have a GitHub issue yet, oldest first.
pub async fn unfiled_sentry_issues(
    db: &PgPool,
    source: &SentrySource,
    token: &str,
    cutoff: &str,
) -> Result<Vec<SentryApiIssue>, AppError> {
    let issues = fetch_sentry_issues(source, token, cutoff).await?;

    let filed: std::collections::HashSet<String> =
        sqlx::query_scalar("SELECT sentry_issue_id FROM sentry_issues WHERE source_id = $1")
            .bind(source.id)
            .fetch_all(db)
            .await?
            .into_iter()
            .collect();

    Ok(issues
        .into_iter()
        .rev()
        .filter(|issue| !filed.contains(&issue.id))
        .collect())
}

// ── Scrubbing ──
// Event data comes from end users and ends up in a GitHub issue and an agent
// prompt, so drop anything that identifies a person or carries a credential.

static SCRUBBERS: LazyLock<Vec<(Regex, &'static str)>> = LazyLock::new(|| {
    [
        (
            r"[\w.+-]+@[A-Za-z][\w-]*(?:\.[\w-]+)*\.[A-Za-z]{2,}",
            "[email]",
        ),
        (r"\beyJ[\w-]+\.[\w-]+\.[\w-]+", "[jwt]"),
        (
            r#"(?i)\b(bearer|token|password|secret|api[_-]?key)([\s:="']+)\S+"#,
            "${1}${2}[redacted]",
        ),
        (r"\b(?:\d{1,3}\.){3}\d{1,3}\b", "[ip]"),
        (
            r"\b[0-9a-fA-F:]{2,}:[0-9a-fA-F:]{2,}:[0-9a-fA-F:]+\b",
            "[ip]",
        ),
        (r"\b[A-Za-z0-9_\-]{32,}\b", "[token]"),
        (r"\+?\d[\d\s-]{7,}\d", "[number]"),
    ]
    .into_iter()
    .map(|(pattern, replacement)| (Regex::new(pattern).unwrap(), replacement))
    .collect()
});

static DIGIT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\d").unwrap());
static API_VERSION: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^v\d+$").unwrap());

fn scrub(text: &str) -> String {
    SCRUBBERS
        .iter()
        .fold(text.to_string(), |acc, (pattern, replacement)| {
            pattern.replace_all(&acc, *replacement).into_owned()
        })
}

/// Keep a URL's shape but not its IDs or slugs: `/venues/main-street/orders/91` → `/venues/:slug/orders/:id`.
fn template_url(url: &str) -> String {
    let Ok(parsed) = reqwest::Url::parse(url) else {
        return scrub(url);
    };
    if parsed.scheme() == "file" {
        return "file://[local file]".into();
    }
    let path: Vec<&str> = parsed
        .path()
        .split('/')
        .map(|segment| {
            if DIGIT.is_match(segment) && !API_VERSION.is_match(segment) {
                ":id"
            } else if segment.contains('-') || segment.len() > 24 {
                ":slug"
            } else {
                segment
            }
        })
        .collect();
    format!(
        "{}://{}{}",
        parsed.scheme(),
        parsed.host_str().unwrap_or(""),
        path.join("/")
    )
}

// ── GitHub issue content ──

fn display_value(value: &Option<serde_json::Value>) -> String {
    match value {
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(serde_json::Value::Null) | None => "?".into(),
        Some(v) => v.to_string(),
    }
}

fn truncate_chars(s: &str, max: usize) -> String {
    match s.char_indices().nth(max) {
        Some((idx, _)) => format!("{}…", &s[..idx]),
        None => s.to_string(),
    }
}

fn date_prefix(s: Option<&str>, len: usize) -> &str {
    let s = s.unwrap_or("");
    s.get(..len).unwrap_or(s)
}

pub fn build_issue_title(issue: &SentryApiIssue) -> String {
    let suffix = format!(" ({})", issue.short_id);
    let room = MAX_TITLE_CHARS.saturating_sub(suffix.chars().count() + 1);
    format!("{}{suffix}", truncate_chars(&scrub(&issue.title), room))
}

fn event_entry<'a>(event: &'a serde_json::Value, kind: &str) -> Option<&'a serde_json::Value> {
    event["entries"]
        .as_array()?
        .iter()
        .find(|e| e["type"].as_str() == Some(kind))
        .map(|e| &e["data"])
}

fn exception_section(data: &serde_json::Value, out: &mut Vec<String>) {
    for value in data["values"].as_array().into_iter().flatten() {
        let handled = if value["mechanism"]["handled"].as_bool() == Some(true) {
            "handled"
        } else {
            "unhandled"
        };
        out.extend([
            "### Error".into(),
            String::new(),
            format!("`{}` ({handled})", value["type"].as_str().unwrap_or("?")),
            String::new(),
            "```".into(),
            scrub(value["value"].as_str().unwrap_or("")),
            "```".into(),
            String::new(),
        ]);

        let frames = value["stacktrace"]["frames"]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        if !frames.is_empty() {
            out.extend([
                "### Stack trace (most recent call last)".into(),
                String::new(),
                "```".into(),
            ]);
            for frame in &frames[frames.len().saturating_sub(MAX_FRAMES)..] {
                let location = frame["filename"]
                    .as_str()
                    .or_else(|| frame["module"].as_str())
                    .unwrap_or("?");
                out.push(format!(
                    "{location}:{} in {}",
                    display_value(&Some(frame["lineNo"].clone())),
                    frame["function"].as_str().unwrap_or("?")
                ));
            }
            out.extend(["```".into(), String::new()]);
        }
    }
}

fn breadcrumb_line(crumb: &serde_json::Value) -> String {
    let data = &crumb["data"];
    let category = crumb["category"]
        .as_str()
        .or_else(|| crumb["type"].as_str())
        .unwrap_or("?");
    let detail = match category {
        "fetch" | "xhr" | "http" => format!(
            "{} {} -> {}",
            data["method"].as_str().unwrap_or(""),
            template_url(data["url"].as_str().unwrap_or("")),
            display_value(&Some(data["status_code"].clone()))
        ),
        "navigation" => format!(
            "{} -> {}",
            template_url(data["from"].as_str().unwrap_or("")),
            template_url(data["to"].as_str().unwrap_or(""))
        ),
        "console" => format!("console.{}", crumb["level"].as_str().unwrap_or("")),
        _ => truncate_chars(&scrub(crumb["message"].as_str().unwrap_or("")), 160),
    };
    let time = crumb["timestamp"]
        .as_str()
        .and_then(|t| t.get(11..19))
        .unwrap_or("");
    format!("{time} {category} {detail}").trim_end().to_string()
}

/// The same layout the sentry-triage skill produced by hand, minus the human summary.
pub fn build_issue_body(issue: &SentryApiIssue, event: Option<&serde_json::Value>) -> String {
    let mut out = vec![
        format!(
            "**Sentry:** [{}]({}) · `{}` · {} events, {} users · first seen {}, last seen {}",
            issue.short_id,
            issue.permalink,
            issue.level.as_deref().unwrap_or("?"),
            display_value(&issue.count),
            display_value(&issue.user_count),
            date_prefix(issue.first_seen.as_deref(), 10),
            date_prefix(issue.last_seen.as_deref(), 10),
        ),
        String::new(),
    ];

    let Some(event) = event else {
        out.push("The latest event could not be loaded; see Sentry for the stack trace.".into());
        return truncate_chars(&out.join("\n"), MAX_BODY_CHARS);
    };

    if let Some(exception) = event_entry(event, "exception") {
        exception_section(exception, &mut out);
    } else {
        let message = event_entry(event, "message")
            .and_then(|m| m["formatted"].as_str())
            .or_else(|| event["message"].as_str())
            .or_else(|| event["title"].as_str())
            .unwrap_or("");
        out.extend([
            "### Error".into(),
            String::new(),
            "```".into(),
            scrub(message),
            "```".into(),
            String::new(),
        ]);
    }

    let tag = |key: &str| -> Option<&str> {
        event["tags"]
            .as_array()?
            .iter()
            .find(|t| t["key"].as_str() == Some(key))?["value"]
            .as_str()
    };
    out.extend([
        "### Latest event".into(),
        String::new(),
        "| Field | Value |".into(),
        "|---|---|".into(),
        format!(
            "| date | {}Z |",
            date_prefix(event["dateCreated"].as_str(), 19)
        ),
    ]);
    for key in EVENT_TAGS {
        if let Some(value) = tag(key) {
            out.push(format!("| {key} | `{}` |", scrub(value)));
        }
    }
    if let Some(url) = tag("url") {
        out.push(format!("| url | `{}` |", template_url(url)));
    }
    if let Some(request) = event_entry(event, "request") {
        if let Some(url) = request["url"].as_str() {
            let method = request["method"].as_str().unwrap_or("");
            out.push(format!(
                "| request | `{}` |",
                format!("{method} {}", template_url(url)).trim_start()
            ));
        }
    }
    out.push(String::new());

    let crumbs = event_entry(event, "breadcrumbs")
        .and_then(|b| b["values"].as_array())
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    if !crumbs.is_empty() {
        let recent = &crumbs[crumbs.len().saturating_sub(MAX_CRUMBS)..];
        out.extend([
            format!("### Breadcrumbs (last {})", recent.len()),
            String::new(),
            "```".into(),
        ]);
        out.extend(recent.iter().map(breadcrumb_line));
        out.extend(["```".into(), String::new()]);
    }

    truncate_chars(&out.join("\n"), MAX_BODY_CHARS)
}

// ── GitHub ──

async fn create_github_issue(
    repo: &str,
    token: &str,
    title: &str,
    body: &str,
    labels: &[String],
) -> Result<(i32, String), AppError> {
    let (owner, repo) = crate::intake::parse_owner_repo(repo)?;
    let resp = reqwest::Client::new()
        .post(format!(
            "https://api.github.com/repos/{owner}/{repo}/issues"
        ))
        .header("Authorization", format!("Bearer {token}"))
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", "tekton-sentry")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .json(&serde_json::json!({ "title": title, "body": body, "labels": labels }))
        .send()
        .await
        .map_err(|e| AppError::Internal(format!("GitHub API request failed: {e}")))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(AppError::Internal(format!(
            "GitHub API returned {status}: {body}"
        )));
    }

    let created: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| AppError::Internal(format!("Failed to parse GitHub response: {e}")))?;
    let number = created["number"].as_i64().unwrap_or(0) as i32;
    let url = created["html_url"].as_str().unwrap_or("").to_string();
    Ok((number, url))
}

// ── Polling ──

/// Claim every enabled source not polled in the last `POLL_INTERVAL_SECS` and
/// poll each in the background. Claiming bumps `last_polled_at` in the same
/// statement, so a slow poll is never picked up again by the next daemon cycle.
pub async fn poll_due_sentry_sources(config: &Arc<Config>, db: &PgPool) -> Result<(), AppError> {
    let due = sqlx::query_as::<_, SentrySourceWithSecret>(&format!(
        "UPDATE sentry_sources SET last_polled_at = NOW() \
         WHERE enabled = true AND (last_polled_at IS NULL \
           OR last_polled_at <= NOW() - $1 * INTERVAL '1 second') \
         RETURNING {SOURCE_COLUMNS}, {SECRET_COLUMNS}"
    ))
    .bind(POLL_INTERVAL_SECS)
    .fetch_all(db)
    .await?;

    for due_source in due {
        let config = config.clone();
        let db = db.clone();
        tokio::spawn(async move {
            let source = &due_source.source;
            let error = match poll_sentry_source(&config, &db, &due_source).await {
                Ok(created) => {
                    if created > 0 {
                        tracing::info!(
                            "Sentry: opened {created} GitHub issue(s) for source '{}' (id={})",
                            source.name,
                            source.id
                        );
                    }
                    None
                }
                Err(e) => {
                    tracing::error!(
                        "Failed to poll Sentry source '{}' (id={}): {e}",
                        source.name,
                        source.id
                    );
                    Some(e.to_string())
                }
            };
            let _ = sqlx::query("UPDATE sentry_sources SET last_error = $1 WHERE id = $2")
                .bind(error)
                .bind(source.id)
                .execute(&db)
                .await;
        });
    }

    Ok(())
}

async fn poll_sentry_source(
    config: &Config,
    db: &PgPool,
    due: &SentrySourceWithSecret,
) -> Result<usize, AppError> {
    let source = &due.source;
    let sentry_token =
        crate::secrets::decrypt_secret(&config.secrets_encryption_key, &due.auth_token_encrypted)?;
    let github_token: String =
        sqlx::query_scalar("SELECT github_token FROM users WHERE github_login = $1")
            .bind(&source.run_as_user)
            .fetch_one(db)
            .await
            .map_err(|_| {
                AppError::Internal(format!(
                    "run_as_user '{}' not found or has no GitHub token",
                    source.run_as_user
                ))
            })?;

    let issues = unfiled_sentry_issues(db, source, &sentry_token, &due.cutoff).await?;
    let mut created = 0;

    for issue in issues.iter().take(MAX_ISSUES_PER_POLL) {
        let event = match sentry_get(
            &format!(
                "organizations/{}/issues/{}/events/latest/",
                source.sentry_org, issue.id
            ),
            &sentry_token,
            &[],
        )
        .await
        {
            Ok(event) => Some(event),
            Err(e) => {
                tracing::warn!("Sentry: no latest event for {}: {e}", issue.short_id);
                None
            }
        };

        let (number, url) = create_github_issue(
            &source.target_repo,
            &github_token,
            &build_issue_title(issue),
            &build_issue_body(issue, event.as_ref()),
            &source.labels,
        )
        .await?;

        sqlx::query(
            "INSERT INTO sentry_issues \
             (source_id, sentry_issue_id, sentry_short_id, sentry_url, title, \
              github_issue_number, github_issue_url) \
             VALUES ($1, $2, $3, $4, $5, $6, $7) \
             ON CONFLICT (source_id, sentry_issue_id) DO NOTHING",
        )
        .bind(source.id)
        .bind(&issue.id)
        .bind(&issue.short_id)
        .bind(&issue.permalink)
        .bind(&issue.title)
        .bind(number)
        .bind(&url)
        .execute(db)
        .await?;

        created += 1;
    }

    Ok(created)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn issue() -> SentryApiIssue {
        serde_json::from_value(json!({
            "id": "4242",
            "shortId": "BACKEND-47",
            "title": "FunctionClauseError: no function clause matching in auth_headers/1",
            "permalink": "https://sentry.io/organizations/acme/issues/4242/",
            "project": { "slug": "backend" },
            "level": "error",
            "count": "17",
            "userCount": 4,
            "firstSeen": "2026-10-26T10:00:00Z",
            "lastSeen": "2026-10-26T11:00:00Z"
        }))
        .unwrap()
    }

    #[test]
    fn title_ends_with_short_id_and_is_bounded() {
        assert_eq!(
            build_issue_title(&issue()),
            "FunctionClauseError: no function clause matching in auth_headers/1 (BACKEND-47)"
        );

        let mut long = issue();
        long.title = "word ".repeat(200);
        let title = build_issue_title(&long);
        assert_eq!(title.chars().count(), MAX_TITLE_CHARS);
        assert!(title.ends_with("… (BACKEND-47)"));
    }

    #[test]
    fn scrub_removes_personal_data_and_credentials() {
        let scrubbed = scrub(
            "user jane.doe@example.com from 10.0.0.12 sent Bearer abc123 \
             with eyJhbGciOiJ.eyJzdWIiOiIx.c2lnbmF0dXJl, phone +54 11 5555-1234",
        );
        assert_eq!(
            scrubbed,
            "user [email] from [ip] sent Bearer [redacted] with [jwt], phone [number]"
        );
    }

    #[test]
    fn template_url_hides_ids_and_slugs() {
        assert_eq!(
            template_url("https://app.example.com/api/v1/venues/main-street/orders/91?x=1"),
            "https://app.example.com/api/v1/venues/:slug/orders/:id"
        );
        assert_eq!(
            template_url("file:///home/jane/app.js"),
            "file://[local file]"
        );
    }

    #[test]
    fn body_has_error_trace_event_and_breadcrumbs() {
        let event = json!({
            "dateCreated": "2026-10-26T11:00:00.123Z",
            "tags": [
                { "key": "environment", "value": "production" },
                { "key": "release", "value": "api@1.4.2" },
                { "key": "url", "value": "https://app.example.com/venues/main-street" }
            ],
            "entries": [
                {
                    "type": "exception",
                    "data": { "values": [{
                        "type": "FunctionClauseError",
                        "value": "no match for jane.doe@example.com",
                        "mechanism": { "handled": false },
                        "stacktrace": { "frames": [
                            { "filename": "lib/router.ex", "function": "call/2", "lineNo": 10 },
                            { "module": "Payments.Client", "function": "auth_headers/1", "lineNo": 42 }
                        ]}
                    }]}
                },
                {
                    "type": "breadcrumbs",
                    "data": { "values": [{
                        "timestamp": "2026-10-26T10:59:58Z",
                        "category": "fetch",
                        "data": { "method": "GET", "url": "https://api.example.com/orders/77", "status_code": 500 }
                    }]}
                }
            ]
        });

        let body = build_issue_body(&issue(), Some(&event));
        assert!(body.starts_with(
            "**Sentry:** [BACKEND-47](https://sentry.io/organizations/acme/issues/4242/) · `error` · \
             17 events, 4 users · first seen 2026-10-26, last seen 2026-10-26"
        ));
        assert!(body.contains("`FunctionClauseError` (unhandled)"));
        assert!(body.contains("no match for [email]"));
        assert!(!body.contains("jane.doe"));
        assert!(body.contains("lib/router.ex:10 in call/2\nPayments.Client:42 in auth_headers/1"));
        assert!(body.contains("| date | 2026-10-26T11:00:00Z |"));
        assert!(body.contains("| environment | `production` |"));
        assert!(body.contains("| url | `https://app.example.com/venues/:slug` |"));
        assert!(body.contains("10:59:58 fetch GET https://api.example.com/orders/:id -> 500"));
    }

    #[test]
    fn body_falls_back_to_message_without_exception() {
        let event = json!({
            "entries": [{ "type": "message", "data": { "formatted": "Payment webhook timed out" } }]
        });
        let body = build_issue_body(&issue(), Some(&event));
        assert!(body.contains("### Error\n\n```\nPayment webhook timed out\n```"));
        assert!(!body.contains("Breadcrumbs"));
    }

    #[test]
    fn body_without_event_points_to_sentry() {
        let body = build_issue_body(&issue(), None);
        assert!(body.contains("could not be loaded"));
    }

    #[test]
    fn body_is_truncated_for_github() {
        let event = json!({
            "entries": [{ "type": "message", "data": { "formatted": "x ".repeat(40_000) } }]
        });
        let body = build_issue_body(&issue(), Some(&event));
        assert_eq!(body.chars().count(), MAX_BODY_CHARS + 1);
    }
}
