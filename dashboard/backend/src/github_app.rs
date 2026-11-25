//! Installation tokens from the Tekton GitHub App, for the backend's own work on task repos:
//! fetching into host mirrors, creating task branches and commits, and opening intake PRs.
//! Each token is limited to one repo and the permissions the caller asks for, and expires
//! after an hour. preview-webhook mints its own tokens from the same App
//! (`server-config/preview-webhook/src/auth.ts`).

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use jsonwebtoken::{Algorithm, EncodingKey, Header};
use tokio::sync::Mutex;

use crate::error::AppError;

/// The repository permissions a token is narrowed to, sorted by name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Access(&'static [(&'static str, &'static str)]);

impl Access {
    /// Fetch a repo into a host mirror.
    pub const READ: Access = Access(&[("contents", "read")]);
    /// Create task branches and commits.
    pub const PUSH: Access = Access(&[("contents", "write")]);
    /// Open a PR, and read the branch diff for its description.
    pub const OPEN_PR: Access = Access(&[("contents", "read"), ("pull_requests", "write")]);

    fn permissions(self) -> serde_json::Map<String, serde_json::Value> {
        self.0
            .iter()
            .map(|(k, v)| (k.to_string(), serde_json::Value::from(*v)))
            .collect()
    }

    fn cache_key(self, repo_name: &str) -> String {
        let perms: Vec<String> = self.0.iter().map(|(k, v)| format!("{k}:{v}")).collect();
        format!("{}|{}", repo_name.to_ascii_lowercase(), perms.join(","))
    }
}

/// A cached token is reused while it has at least this long left. Covers the longest single
/// use: a mirror fetch can run for 10 minutes (`mirror::GIT_TIMEOUT`).
const MIN_TTL: Duration = Duration::from_secs(15 * 60);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);

struct CachedToken {
    token: String,
    expires_at: DateTime<Utc>,
}

pub struct GitHubApp {
    app_id: String,
    installation_id: String,
    key: EncodingKey,
    api_url: String,
    client: reqwest::Client,
    /// The installation's account. Repos owned by anyone else are refused.
    account: Mutex<Option<String>>,
    tokens: Mutex<HashMap<String, CachedToken>>,
}

impl GitHubApp {
    /// Reads `GITHUB_APP_ID`, `GITHUB_APP_INSTALLATION_ID` and `GITHUB_APP_PRIVATE_KEY_PATH`.
    /// `None` when none is set (local development and e2e tests, which never reach GitHub);
    /// an error when only some are, or the key can't be used. Errors never include key material.
    pub fn from_env() -> anyhow::Result<Option<Arc<Self>>> {
        let var = |name: &str| std::env::var(name).ok().filter(|v| !v.trim().is_empty());
        let (app_id, installation_id, key_path) = match (
            var("GITHUB_APP_ID"),
            var("GITHUB_APP_INSTALLATION_ID"),
            var("GITHUB_APP_PRIVATE_KEY_PATH"),
        ) {
            (None, None, None) => return Ok(None),
            (Some(a), Some(i), Some(k)) => (a, i, k),
            _ => anyhow::bail!(
                "Set all of GITHUB_APP_ID, GITHUB_APP_INSTALLATION_ID and \
                 GITHUB_APP_PRIVATE_KEY_PATH, or none of them"
            ),
        };
        let pem = std::fs::read(&key_path).map_err(|e| {
            anyhow::anyhow!(
                "Cannot read the GitHub App private key at {key_path} ({})",
                e.kind()
            )
        })?;
        let app = Self::new(
            app_id.trim(),
            installation_id.trim(),
            &pem,
            "https://api.github.com",
        )
        .map_err(|e| anyhow::anyhow!("{e} (key at {key_path})"))?;
        Ok(Some(Arc::new(app)))
    }

    fn new(app_id: &str, installation_id: &str, pem: &[u8], api_url: &str) -> anyhow::Result<Self> {
        let numeric = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
        if !numeric(app_id) || !numeric(installation_id) {
            anyhow::bail!("GITHUB_APP_ID and GITHUB_APP_INSTALLATION_ID must be numeric");
        }
        let key = EncodingKey::from_rsa_pem(pem).map_err(|_| {
            anyhow::anyhow!("The GitHub App private key is not an RSA PEM private key")
        })?;
        // Check that the key signs, so a bad key fails at startup rather than on the first task.
        jsonwebtoken::encode(&Header::new(Algorithm::RS256), &serde_json::json!({}), &key)
            .map_err(|_| anyhow::anyhow!("The GitHub App private key can't sign RS256 JWTs"))?;
        Ok(Self {
            app_id: app_id.to_string(),
            installation_id: installation_id.to_string(),
            key,
            api_url: api_url.trim_end_matches('/').to_string(),
            client: reqwest::Client::new(),
            account: Mutex::new(None),
            tokens: Mutex::new(HashMap::new()),
        })
    }

    /// An installation token for `repo` ("owner/name") with `access`, good for at least
    /// [`MIN_TTL`]. Tokens are cached per repo and access.
    pub async fn token(&self, repo: &str, access: Access) -> Result<String, AppError> {
        let (owner, name) = split_repo(repo)
            .ok_or_else(|| AppError::BadRequest(format!("Invalid repository '{repo}'")))?;

        let account = self.account().await?;
        if !owner.eq_ignore_ascii_case(&account) {
            return Err(AppError::Forbidden(format!(
                "'{repo}' isn't owned by the GitHub App installation's account ({account})"
            )));
        }

        let key = access.cache_key(name);
        // Held while minting, so concurrent callers share one new token.
        let mut tokens = self.tokens.lock().await;
        if let Some(cached) = tokens.get(&key) {
            if cached.expires_at - Utc::now() >= chrono::Duration::from_std(MIN_TTL).unwrap() {
                return Ok(cached.token.clone());
            }
        }
        let minted = self.mint(name, access).await?;
        tracing::info!(
            "Minted GitHub App token for {repo} ({}; expires {})",
            access.cache_key(name),
            minted.expires_at
        );
        let token = minted.token.clone();
        tokens.insert(key, minted);
        Ok(token)
    }

    async fn account(&self) -> Result<String, AppError> {
        let mut account = self.account.lock().await;
        if let Some(login) = account.as_ref() {
            return Ok(login.clone());
        }
        let resp = self
            .client
            .get(format!(
                "{}/app/installations/{}",
                self.api_url, self.installation_id
            ))
            .timeout(REQUEST_TIMEOUT)
            .headers(self.app_headers()?)
            .send()
            .await
            .map_err(|e| self.request_failed("look up the App installation", e))?;
        let body = read_json(resp, "Looking up the GitHub App installation").await?;
        let login = body["account"]["login"]
            .as_str()
            .filter(|l| !l.is_empty())
            .ok_or_else(|| {
                AppError::Internal("The GitHub App installation has no account login".into())
            })?
            .to_string();
        *account = Some(login.clone());
        Ok(login)
    }

    async fn mint(&self, repo_name: &str, access: Access) -> Result<CachedToken, AppError> {
        let resp = self
            .client
            .post(format!(
                "{}/app/installations/{}/access_tokens",
                self.api_url, self.installation_id
            ))
            .timeout(REQUEST_TIMEOUT)
            .headers(self.app_headers()?)
            .json(&serde_json::json!({
                "repositories": [repo_name],
                "permissions": access.permissions(),
            }))
            .send()
            .await
            .map_err(|e| self.request_failed("mint an installation token", e))?;
        let body = read_json(resp, "Minting a GitHub App installation token").await?;
        let token = body["token"].as_str().filter(|t| !t.is_empty());
        let expires_at = body["expires_at"]
            .as_str()
            .and_then(|s| DateTime::parse_from_rfc3339(s).ok());
        match (token, expires_at) {
            (Some(token), Some(expires_at)) => Ok(CachedToken {
                token: token.to_string(),
                expires_at: expires_at.with_timezone(&Utc),
            }),
            _ => Err(AppError::Internal(
                "GitHub's installation token response is missing token or expires_at".into(),
            )),
        }
    }

    fn app_headers(&self) -> Result<reqwest::header::HeaderMap, AppError> {
        let now = Utc::now().timestamp();
        let claims = serde_json::json!({
            "iat": now - 60, // GitHub allows for clock drift this way
            "exp": now + 5 * 60,
            "iss": self.app_id,
        });
        let jwt = jsonwebtoken::encode(&Header::new(Algorithm::RS256), &claims, &self.key)
            .map_err(|e| AppError::Internal(format!("Could not sign the GitHub App JWT: {e}")))?;
        let mut headers = reqwest::header::HeaderMap::new();
        let mut insert = |name: &'static str, value: String| {
            if let Ok(v) = reqwest::header::HeaderValue::from_str(&value) {
                headers.insert(name, v);
            }
        };
        insert("authorization", format!("Bearer {jwt}"));
        insert("accept", "application/vnd.github+json".into());
        insert("x-github-api-version", "2022-11-28".into());
        insert("user-agent", "tekton-dashboard".into());
        Ok(headers)
    }

    fn request_failed(&self, what: &str, e: reqwest::Error) -> AppError {
        AppError::Internal(format!(
            "Could not {what} for GitHub App {}: {e}",
            self.app_id
        ))
    }
}

fn split_repo(repo: &str) -> Option<(&str, &str)> {
    let part_ok = |s: &str| {
        !s.is_empty()
            && !s.starts_with('.')
            && s.chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
    };
    repo.split_once('/')
        .filter(|(owner, name)| part_ok(owner) && part_ok(name))
}

/// The JSON body of a successful response, or an error with GitHub's `message`.
async fn read_json(resp: reqwest::Response, what: &str) -> Result<serde_json::Value, AppError> {
    let status = resp.status();
    let body: serde_json::Value = resp.json().await.unwrap_or_default();
    if !status.is_success() {
        let message = body["message"].as_str().unwrap_or("no message");
        return Err(AppError::Internal(format!(
            "{what} failed: GitHub returned {status}: {message}"
        )));
    }
    Ok(body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::extract::State;
    use axum::http::{HeaderMap, StatusCode};
    use axum::routing::{get, post};
    use axum::{Json, Router};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex as StdMutex;

    const KEY: &[u8] = include_bytes!("testdata/github_app_test_key.pem");
    const PUBLIC_KEY: &[u8] = include_bytes!("testdata/github_app_test_key.pub.pem");

    #[derive(Clone, Default)]
    struct Mock {
        lookups: Arc<AtomicUsize>,
        mints: Arc<AtomicUsize>,
        bodies: Arc<StdMutex<Vec<serde_json::Value>>>,
        /// Lifetime of minted tokens, in seconds.
        ttl: Arc<AtomicUsize>,
        fail_mint: Arc<std::sync::atomic::AtomicBool>,
    }

    fn check_jwt(headers: &HeaderMap) -> bool {
        let Some(jwt) = headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
        else {
            return false;
        };
        let mut validation = jsonwebtoken::Validation::new(Algorithm::RS256);
        validation.set_issuer(&["123"]);
        validation.set_required_spec_claims(&["exp", "iat", "iss"]);
        jsonwebtoken::decode::<serde_json::Value>(
            jwt,
            &jsonwebtoken::DecodingKey::from_rsa_pem(PUBLIC_KEY).unwrap(),
            &validation,
        )
        .is_ok()
    }

    async fn installation(
        State(m): State<Mock>,
        headers: HeaderMap,
    ) -> (StatusCode, Json<serde_json::Value>) {
        if !check_jwt(&headers) {
            return (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({ "message": "bad jwt" })),
            );
        }
        m.lookups.fetch_add(1, Ordering::SeqCst);
        (
            StatusCode::OK,
            Json(serde_json::json!({ "account": { "login": "TestOrg" } })),
        )
    }

    async fn access_tokens(
        State(m): State<Mock>,
        headers: HeaderMap,
        Json(body): Json<serde_json::Value>,
    ) -> (StatusCode, Json<serde_json::Value>) {
        if !check_jwt(&headers) {
            return (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({ "message": "bad jwt" })),
            );
        }
        if m.fail_mint.load(Ordering::SeqCst) {
            return (
                StatusCode::UNPROCESSABLE_ENTITY,
                Json(serde_json::json!({ "message": "repository not installed" })),
            );
        }
        let n = m.mints.fetch_add(1, Ordering::SeqCst);
        m.bodies.lock().unwrap().push(body);
        let ttl = m.ttl.load(Ordering::SeqCst) as i64;
        let expires_at = (Utc::now() + chrono::Duration::seconds(ttl)).to_rfc3339();
        (
            StatusCode::CREATED,
            Json(serde_json::json!({ "token": format!("ghs_test{n}"), "expires_at": expires_at })),
        )
    }

    async fn start(mock: Mock) -> GitHubApp {
        mock.ttl.store(3600, Ordering::SeqCst);
        let app = Router::new()
            .route("/app/installations/42", get(installation))
            .route("/app/installations/42/access_tokens", post(access_tokens))
            .with_state(mock);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        GitHubApp::new("123", "42", KEY, &format!("http://{addr}")).unwrap()
    }

    #[tokio::test]
    async fn mints_a_token_narrowed_to_one_repo() {
        let mock = Mock::default();
        let app = start(mock.clone()).await;

        let token = app.token("testorg/widgets", Access::OPEN_PR).await.unwrap();
        assert_eq!(token, "ghs_test0");
        let bodies = mock.bodies.lock().unwrap();
        assert_eq!(
            bodies[0],
            serde_json::json!({
                "repositories": ["widgets"],
                "permissions": { "contents": "read", "pull_requests": "write" },
            })
        );
    }

    #[tokio::test]
    async fn reuses_tokens_per_repo_and_access() {
        let mock = Mock::default();
        let app = start(mock.clone()).await;

        let a = app.token("testorg/widgets", Access::PUSH).await.unwrap();
        let b = app.token("TestOrg/Widgets", Access::PUSH).await.unwrap();
        assert_eq!(a, b);
        assert_eq!(mock.mints.load(Ordering::SeqCst), 1);

        app.token("testorg/widgets", Access::READ).await.unwrap();
        app.token("testorg/gadgets", Access::PUSH).await.unwrap();
        assert_eq!(mock.mints.load(Ordering::SeqCst), 3);
        // The installation's account is looked up once.
        assert_eq!(mock.lookups.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn mints_again_when_the_cached_token_is_close_to_expiring() {
        let mock = Mock::default();
        let app = start(mock.clone()).await;
        mock.ttl.store(10 * 60, Ordering::SeqCst);

        let a = app.token("testorg/widgets", Access::PUSH).await.unwrap();
        let b = app.token("testorg/widgets", Access::PUSH).await.unwrap();
        assert_ne!(a, b);
        assert_eq!(mock.mints.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn refuses_repos_outside_the_installation_account() {
        let mock = Mock::default();
        let app = start(mock.clone()).await;

        let err = app
            .token("someone-else/widgets", Access::READ)
            .await
            .unwrap_err();
        assert!(matches!(err, AppError::Forbidden(_)), "{err}");
        let err = app.token("testorg", Access::READ).await.unwrap_err();
        assert!(matches!(err, AppError::BadRequest(_)), "{err}");
        let err = app.token("testorg/../x", Access::READ).await.unwrap_err();
        assert!(matches!(err, AppError::BadRequest(_)), "{err}");
        assert_eq!(mock.mints.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn reports_github_errors() {
        let mock = Mock::default();
        let app = start(mock.clone()).await;
        mock.fail_mint.store(true, Ordering::SeqCst);

        let err = app
            .token("testorg/widgets", Access::PUSH)
            .await
            .unwrap_err();
        assert!(
            err.to_string().contains("repository not installed"),
            "{err}"
        );
    }

    #[test]
    fn rejects_bad_settings() {
        assert!(GitHubApp::new("abc", "42", KEY, "http://x").is_err());
        assert!(GitHubApp::new("123", "", KEY, "http://x").is_err());
        let err = GitHubApp::new("123", "42", b"not a key", "http://x")
            .err()
            .unwrap();
        assert!(err.to_string().contains("not an RSA PEM"), "{err}");
    }
}
