//! Slack notifications for intake work.
//!
//! When an intake issue is picked up, its source's channel gets a message for
//! it; every later change (follow-up, review, PR, done, failed) is a reply in
//! that message's thread, and the parent is edited to show the current status.
//!
//! Nothing here reacts to events. Each issue stores what was last announced in
//! its thread, and every cycle posts the difference between that and the
//! issue's current state. A Slack call that fails is retried on the next
//! cycle, unless the thread is gone for good. The notifier runs in its own
//! loop, so Slack being slow or down never holds up intake itself.

use std::sync::LazyLock;
use std::time::Duration;

use sqlx::PgPool;

use crate::config::Config;
use crate::error::AppError;

/// Statuses that are announced; `backlog` and `pending` are not intake work yet
const ANNOUNCED_STATUSES: &str = "('task_created', 'review', 'done', 'failed')";

static HTTP: LazyLock<reqwest::Client> = LazyLock::new(|| {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .expect("reqwest client")
});

// ── Slack Web API ──

/// API errors no retry fixes: the thread or its channel is gone, or the bot
/// was removed from it
const PERMANENT_ERRORS: [&str; 4] = [
    "thread_not_found",
    "channel_not_found",
    "is_archived",
    "not_in_channel",
];

#[derive(Debug)]
enum SlackError {
    /// Slack answered with `ok: false` and this error code
    Api {
        method: String,
        error: String,
    },
    Other(String),
}

impl SlackError {
    fn is_permanent(&self) -> bool {
        matches!(self, Self::Api { error, .. } if PERMANENT_ERRORS.contains(&error.as_str()))
    }
}

impl std::fmt::Display for SlackError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Api { method, error } => write!(f, "Slack {method}: {error}"),
            Self::Other(message) => f.write_str(message),
        }
    }
}

impl From<SlackError> for AppError {
    fn from(e: SlackError) -> Self {
        AppError::Internal(e.to_string())
    }
}

struct SlackClient<'a> {
    api_url: &'a str,
    token: String,
}

impl SlackClient<'_> {
    async fn call(
        &self,
        method: &str,
        body: serde_json::Value,
    ) -> Result<serde_json::Value, SlackError> {
        let resp = HTTP
            .post(format!("{}/{method}", self.api_url.trim_end_matches('/')))
            .bearer_auth(&self.token)
            .json(&body)
            .send()
            .await
            .map_err(|e| SlackError::Other(format!("Slack {method}: {e}")))?;
        let status = resp.status();
        let json: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| SlackError::Other(format!("Slack {method}: HTTP {status}: {e}")))?;
        // Slack answers 200 with ok=false for API errors (bad token, unknown channel...)
        if json["ok"].as_bool() != Some(true) {
            return Err(SlackError::Api {
                method: method.into(),
                error: json["error"].as_str().unwrap_or("unknown error").into(),
            });
        }
        Ok(json)
    }

    /// Posts a message, in `thread_ts`'s thread if given, and returns its `ts`
    async fn post_message(
        &self,
        channel: &str,
        text: &str,
        thread_ts: Option<&str>,
    ) -> Result<String, SlackError> {
        let mut body = serde_json::json!({
            "channel": channel,
            "text": text,
            "unfurl_links": false,
            "unfurl_media": false,
        });
        if let Some(ts) = thread_ts {
            body["thread_ts"] = ts.into();
        }
        let json = self.call("chat.postMessage", body).await?;
        json["ts"]
            .as_str()
            .map(String::from)
            .ok_or_else(|| SlackError::Other("Slack chat.postMessage: no ts in response".into()))
    }

    async fn update_message(&self, channel: &str, ts: &str, text: &str) -> Result<(), SlackError> {
        self.call(
            "chat.update",
            serde_json::json!({ "channel": channel, "ts": ts, "text": text }),
        )
        .await?;
        Ok(())
    }
}

// ── What to announce ──

/// The parts of an issue that its thread announces
#[derive(Debug, Clone, Default, PartialEq)]
struct Announced {
    task_id: Option<String>,
    status: Option<String>,
    pr_url: Option<String>,
}

#[derive(Debug, PartialEq)]
enum Reply {
    /// A retried issue got a new task
    PickedUpAgain,
    PrOpened,
    Status,
}

/// Thread replies that bring a thread announcing `announced` up to `current`
fn replies(current: &Announced, announced: &Announced) -> Vec<Reply> {
    let mut out = Vec::new();
    let new_task = current.task_id.is_some() && current.task_id != announced.task_id;
    if new_task {
        out.push(Reply::PickedUpAgain);
    }
    if current.pr_url.is_some() && current.pr_url != announced.pr_url {
        out.push(Reply::PrOpened);
    }
    // A new task's "in progress" is what PickedUpAgain already says
    let restated = new_task && current.status.as_deref() == Some("task_created");
    if current.status != announced.status && !restated {
        out.push(Reply::Status);
    }
    out
}

/// What `announced` becomes once `reply` is posted
fn apply(reply: &Reply, current: &Announced, announced: &mut Announced) {
    match reply {
        Reply::PickedUpAgain => {
            announced.task_id = current.task_id.clone();
            if current.status.as_deref() == Some("task_created") {
                announced.status = current.status.clone();
            }
        }
        Reply::PrOpened => announced.pr_url = current.pr_url.clone(),
        Reply::Status => announced.status = current.status.clone(),
    }
}

// ── Message text ──

#[derive(sqlx::FromRow)]
struct IssueRow {
    issue_id: i64,
    external_title: String,
    external_url: Option<String>,
    status: String,
    task_id: Option<String>,
    pr_url: Option<String>,
    pr_number: Option<i32>,
    slack_channel: Option<String>,
    slack_thread_ts: Option<String>,
    slack_notified_task_id: Option<String>,
    slack_notified_status: Option<String>,
    slack_notified_pr_url: Option<String>,
    source_name: String,
    target_repo: String,
    source_channel: String,
    token_encrypted: String,
}

impl IssueRow {
    fn current(&self) -> Announced {
        Announced {
            task_id: self.task_id.clone(),
            status: Some(self.status.clone()),
            pr_url: self.pr_url.clone(),
        }
    }

    fn announced(&self) -> Announced {
        Announced {
            task_id: self.slack_notified_task_id.clone(),
            status: self.slack_notified_status.clone(),
            pr_url: self.slack_notified_pr_url.clone(),
        }
    }
}

/// Escapes the characters Slack's mrkdwn treats as markup
fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// `<url|label>`, or the bare label when `url` isn't http(s). URLs come from
/// providers and tasks, so whatever could end the link early and inject
/// markup (mentions, other links) is encoded.
fn link(url: &str, label: &str) -> String {
    let label = escape(label);
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return label;
    }
    let mut safe = String::with_capacity(url.len());
    for c in url.chars() {
        match c {
            '<' => safe.push_str("%3C"),
            '>' => safe.push_str("%3E"),
            '|' => safe.push_str("%7C"),
            '&' => safe.push_str("&amp;"),
            c if c.is_whitespace() || c.is_control() => {
                let mut buf = [0u8; 4];
                for b in c.encode_utf8(&mut buf).bytes() {
                    safe.push_str(&format!("%{b:02X}"));
                }
            }
            c => safe.push(c),
        }
    }
    format!("<{safe}|{label}>")
}

fn status_label(status: &str) -> (&'static str, &'static str) {
    match status {
        "task_created" => (":arrows_counterclockwise:", "In progress"),
        "review" => (":eyes:", "Ready for review"),
        "done" => (":white_check_mark:", "Done"),
        "failed" => (":x:", "Failed"),
        _ => (":grey_question:", "Unknown"),
    }
}

/// The PR, linked only when it's on GitHub: a task's PR URL can be set by hand
fn pr_link(row: &IssueRow) -> String {
    let label = match row.pr_number {
        Some(n) if n > 0 => format!("PR #{n}"),
        _ => "PR".into(),
    };
    match row.pr_url.as_deref() {
        Some(url) if url.starts_with("https://github.com/") => link(url, &label),
        _ => escape(&label),
    }
}

fn task_link(dashboard_url: &str, task_id: &str) -> String {
    link(&format!("{dashboard_url}/tasks/{task_id}"), "task")
}

fn parent_text(row: &IssueRow, dashboard_url: &str) -> String {
    let (emoji, label) = status_label(&row.status);
    let title = match &row.external_url {
        Some(url) if !url.is_empty() => link(url, &row.external_title),
        _ => escape(&row.external_title),
    };
    let mut details = vec![
        format!("`{}`", escape(&row.target_repo)),
        format!("via {}", escape(&row.source_name)),
    ];
    if let Some(task_id) = &row.task_id {
        details.push(task_link(dashboard_url, task_id));
    }
    if row.pr_url.is_some() {
        details.push(pr_link(row));
    }
    format!("{emoji} *{label}*: {title}\n{}", details.join(" · "))
}

fn reply_text(reply: &Reply, row: &IssueRow, dashboard_url: &str) -> String {
    match reply {
        Reply::PickedUpAgain => {
            let task = row.task_id.as_deref().unwrap_or_default();
            format!(
                ":arrows_counterclockwise: Picked up again as a new {}",
                task_link(dashboard_url, task)
            )
        }
        Reply::PrOpened => format!(":link: Opened {}", pr_link(row)),
        Reply::Status => match row.status.as_str() {
            "task_created" => {
                ":arrows_counterclockwise: Back in progress: a follow-up was sent".into()
            }
            "review" => ":eyes: Ready for review".into(),
            "done" => ":white_check_mark: Done".into(),
            // Errors can carry tokens, paths and hostnames, and the channel
            // reaches more people than the dashboard, so they stay there
            "failed" => match &row.task_id {
                Some(task_id) => format!(
                    ":x: Failed: details on the {}",
                    task_link(dashboard_url, task_id)
                ),
                None => ":x: Failed".into(),
            },
            other => format!("Status: {}", escape(other)),
        },
    }
}

// ── Notifier ──

/// Posts every pending Slack update for intake issues. Called every cycle of
/// the notifier loop; a failure on one issue is logged and retried next cycle.
pub async fn notify_intake_updates(config: &Config, db: &PgPool) -> Result<(), AppError> {
    // An issue without a thread gets one when it has a task the thread would
    // be about, or when it failed before getting one. `slack_notified_*` start
    // out as what an issue already had when Slack was set up, so existing work
    // isn't announced.
    let rows = sqlx::query_as::<_, IssueRow>(&format!(
        "SELECT i.id AS issue_id, i.external_title, i.external_url, i.status, \
         i.task_id, t.pr_url, t.pr_number, \
         i.slack_channel, i.slack_thread_ts, i.slack_notified_task_id, \
         i.slack_notified_status, i.slack_notified_pr_url, \
         s.name AS source_name, s.target_repo, \
         s.slack_channel_id AS source_channel, \
         s.slack_bot_token_encrypted AS token_encrypted \
         FROM intake_issues i \
         JOIN intake_sources s ON s.id = i.source_id \
         LEFT JOIN tasks t ON t.id = i.task_id \
         WHERE s.slack_channel_id <> '' AND s.slack_bot_token_encrypted IS NOT NULL \
         AND i.status IN {ANNOUNCED_STATUSES} \
         AND CASE WHEN i.slack_thread_ts IS NULL \
             THEN (i.task_id IS NOT NULL AND i.task_id IS DISTINCT FROM i.slack_notified_task_id) \
               OR (i.task_id IS NULL AND i.status = 'failed' \
                   AND i.slack_notified_status IS DISTINCT FROM 'failed') \
             ELSE i.status IS DISTINCT FROM i.slack_notified_status \
               OR i.task_id IS DISTINCT FROM i.slack_notified_task_id \
               OR (t.pr_url IS NOT NULL AND t.pr_url IS DISTINCT FROM i.slack_notified_pr_url) \
         END \
         ORDER BY i.id"
    ))
    .fetch_all(db)
    .await?;

    let dashboard_url = config.dashboard_url();
    for row in &rows {
        if let Err(e) = notify_issue(config, db, row, &dashboard_url).await {
            tracing::warn!(
                "Slack: update for intake issue {} failed: {e}",
                row.issue_id
            );
        }
    }
    Ok(())
}

async fn notify_issue(
    config: &Config,
    db: &PgPool,
    row: &IssueRow,
    dashboard_url: &str,
) -> Result<(), AppError> {
    let token =
        crate::secrets::decrypt_secret(&config.secrets_encryption_key, &row.token_encrypted)?;
    let client = SlackClient {
        api_url: &config.slack_api_url,
        token,
    };
    let current = row.current();
    let text = parent_text(row, dashboard_url);

    let Some(thread_ts) = &row.slack_thread_ts else {
        let ts = client
            .post_message(&row.source_channel, &text, None)
            .await?;
        // One write for the thread and what it announced: a thread saved
        // without its state would read as a retry on the next cycle
        sqlx::query(
            "UPDATE intake_issues SET slack_channel = $2, slack_thread_ts = $3, \
             slack_notified_task_id = $4, slack_notified_status = $5, \
             slack_notified_pr_url = $6 WHERE id = $1",
        )
        .bind(row.issue_id)
        .bind(&row.source_channel)
        .bind(&ts)
        .bind(&current.task_id)
        .bind(&current.status)
        .bind(&current.pr_url)
        .execute(db)
        .await?;
        tracing::info!("Slack: announced intake issue {}", row.issue_id);
        return Ok(());
    };

    // A thread stays in the channel it started in, even if the source moves
    let channel = row.slack_channel.as_deref().unwrap_or(&row.source_channel);
    // The thread still carries the news if the parent can't be edited
    // (deleted, or posted by another bot)
    if let Err(e) = client.update_message(channel, thread_ts, &text).await {
        tracing::warn!(
            "Slack: couldn't update the message for intake issue {}: {e}",
            row.issue_id
        );
    }

    let mut announced = row.announced();
    for reply in replies(&current, &announced) {
        if let Err(e) = client
            .post_message(
                channel,
                &reply_text(&reply, row, dashboard_url),
                Some(thread_ts),
            )
            .await
        {
            if !e.is_permanent() {
                return Err(e.into());
            }
            // Counts as announced, so it isn't retried every cycle forever
            tracing::warn!(
                "Slack: giving up on the thread for intake issue {}: {e}",
                row.issue_id
            );
            return save_announced(db, row.issue_id, &current).await;
        }
        // Saved per reply, so a later failure doesn't repeat this one
        apply(&reply, &current, &mut announced);
        save_announced(db, row.issue_id, &announced).await?;
    }
    // Changes with nothing to say (a retry that is still waiting for its task)
    if announced != current {
        save_announced(db, row.issue_id, &current).await?;
    }
    Ok(())
}

async fn save_announced(db: &PgPool, issue_id: i64, announced: &Announced) -> Result<(), AppError> {
    sqlx::query(
        "UPDATE intake_issues SET slack_notified_task_id = $2, slack_notified_status = $3, \
         slack_notified_pr_url = $4 WHERE id = $1",
    )
    .bind(issue_id)
    .bind(&announced.task_id)
    .bind(&announced.status)
    .bind(&announced.pr_url)
    .execute(db)
    .await?;
    Ok(())
}

/// Marks a source's current tasks, and its issues that failed without one, as
/// already announced, so turning Slack on for a source only announces work
/// picked up from then on.
pub async fn skip_existing_work(db: &PgPool, source_id: i64) -> Result<(), AppError> {
    sqlx::query(
        "UPDATE intake_issues SET slack_notified_task_id = task_id, \
         slack_notified_status = CASE WHEN task_id IS NULL THEN status \
             ELSE slack_notified_status END \
         WHERE source_id = $1 AND slack_thread_ts IS NULL \
         AND (task_id IS NOT NULL OR status = 'failed')",
    )
    .bind(source_id)
    .execute(db)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(task: Option<&str>, status: &str, pr: Option<&str>) -> Announced {
        Announced {
            task_id: task.map(String::from),
            status: Some(status.into()),
            pr_url: pr.map(String::from),
        }
    }

    fn row(status: &str) -> IssueRow {
        IssueRow {
            issue_id: 7,
            external_title: "Fix <script> & stuff".into(),
            external_url: Some("https://github.com/o/r/issues/1".into()),
            status: status.into(),
            task_id: Some("abc123".into()),
            pr_url: None,
            pr_number: None,
            slack_channel: None,
            slack_thread_ts: None,
            slack_notified_task_id: None,
            slack_notified_status: None,
            slack_notified_pr_url: None,
            source_name: "Bugs".into(),
            target_repo: "o/r".into(),
            source_channel: "C123".into(),
            token_encrypted: String::new(),
        }
    }

    #[test]
    fn nothing_to_say_when_up_to_date() {
        let s = state(Some("t1"), "review", Some("https://pr/1"));
        assert!(replies(&s, &s).is_empty());
    }

    #[test]
    fn status_changes_are_announced() {
        let before = state(Some("t1"), "task_created", None);
        let after = state(Some("t1"), "review", None);
        assert_eq!(replies(&after, &before), vec![Reply::Status]);
    }

    #[test]
    fn pr_is_announced_before_review() {
        let before = state(Some("t1"), "task_created", None);
        let after = state(Some("t1"), "review", Some("https://pr/1"));
        assert_eq!(
            replies(&after, &before),
            vec![Reply::PrOpened, Reply::Status]
        );
    }

    #[test]
    fn pr_is_announced_once() {
        let before = state(Some("t1"), "review", Some("https://pr/1"));
        let after = state(Some("t1"), "done", Some("https://pr/1"));
        assert_eq!(replies(&after, &before), vec![Reply::Status]);
    }

    #[test]
    fn retry_with_a_new_task_says_picked_up_again_only() {
        let before = state(Some("t1"), "failed", Some("https://pr/1"));
        let after = state(Some("t2"), "task_created", None);
        assert_eq!(replies(&after, &before), vec![Reply::PickedUpAgain]);
    }

    #[test]
    fn retry_whose_task_already_failed_says_both() {
        let before = state(Some("t1"), "review", None);
        let after = state(Some("t2"), "failed", None);
        assert_eq!(
            replies(&after, &before),
            vec![Reply::PickedUpAgain, Reply::Status]
        );
    }

    #[test]
    fn retry_that_fails_without_a_task_says_failed() {
        // A retry clears the announced status
        let before = Announced {
            task_id: Some("t1".into()),
            ..Default::default()
        };
        let after = state(None, "failed", None);
        assert_eq!(replies(&after, &before), vec![Reply::Status]);
        let mut r = row("failed");
        r.task_id = None;
        assert_eq!(reply_text(&Reply::Status, &r, "https://d"), ":x: Failed");
    }

    #[test]
    fn only_a_lost_thread_is_given_up_on() {
        let api = |error: &str| SlackError::Api {
            method: "chat.postMessage".into(),
            error: error.into(),
        };
        assert!(api("thread_not_found").is_permanent());
        assert!(api("not_in_channel").is_permanent());
        assert!(!api("invalid_auth").is_permanent());
        assert!(!api("ratelimited").is_permanent());
        assert!(!SlackError::Other("timed out".into()).is_permanent());
    }

    #[test]
    fn applying_every_reply_reaches_current() {
        let before = state(Some("t1"), "failed", Some("https://pr/1"));
        let after = state(Some("t2"), "task_created", Some("https://pr/2"));
        let mut announced = before.clone();
        for reply in replies(&after, &before) {
            apply(&reply, &after, &mut announced);
        }
        assert_eq!(announced, after);
    }

    #[test]
    fn parent_links_issue_task_and_pr() {
        let mut r = row("review");
        r.pr_url = Some("https://github.com/o/r/pull/9".into());
        r.pr_number = Some(9);
        assert_eq!(
            parent_text(&r, "https://dash.example.com"),
            ":eyes: *Ready for review*: <https://github.com/o/r/issues/1|Fix &lt;script&gt; &amp; stuff>\n\
             `o/r` · via Bugs · <https://dash.example.com/tasks/abc123|task> · \
             <https://github.com/o/r/pull/9|PR #9>"
        );
    }

    #[test]
    fn failure_reply_links_the_task_instead_of_the_error() {
        assert_eq!(
            reply_text(&Reply::Status, &row("failed"), "https://d"),
            ":x: Failed: details on the <https://d/tasks/abc123|task>"
        );
    }

    #[test]
    fn urls_cannot_break_out_of_a_link() {
        assert_eq!(
            link(
                "https://x|Approved> <!channel> <https://evil.example|Click",
                "PR"
            ),
            "<https://x%7CApproved%3E%20%3C!channel%3E%20%3Chttps://evil.example%7CClick|PR>"
        );
        assert_eq!(
            link("https://x/?a=1&b=2", "l"),
            "<https://x/?a=1&amp;b=2|l>"
        );
    }

    #[test]
    fn only_http_urls_are_linked() {
        assert_eq!(link("javascript:alert(1)", "t"), "t");
        assert_eq!(link("slack://open", "<t>"), "&lt;t&gt;");
    }

    #[test]
    fn pr_is_linked_only_on_github() {
        let mut r = row("review");
        r.pr_number = Some(3);
        r.pr_url = Some("https://evil.example/pull/3".into());
        assert_eq!(pr_link(&r), "PR #3");
        r.pr_url = Some("https://github.com/o/r/pull/3".into());
        assert_eq!(pr_link(&r), "<https://github.com/o/r/pull/3|PR #3>");
    }

    #[test]
    fn follow_up_reply() {
        assert_eq!(
            reply_text(&Reply::Status, &row("task_created"), "https://d"),
            ":arrows_counterclockwise: Back in progress: a follow-up was sent"
        );
    }
}
