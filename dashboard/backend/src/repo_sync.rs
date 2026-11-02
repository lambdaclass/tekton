//! Host-side git access for agent containers. This is the only place a GitHub token meets git.
//!
//! The host keeps one bare cache repo per GitHub repo, fetches into it with the token passed
//! through `GIT_CONFIG_*` env vars (never argv, never disk), and hands agents git bundles.
//! Agents only ever see the plain `https://github.com/...` URL. The host never runs git inside
//! an agent-writable repo: a hostile `.git/config` there could run code as root.

use base64::Engine;
use dashmap::DashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, LazyLock};
use tokio::process::Command;
use tokio::sync::Mutex;

use crate::error::AppError;
use crate::shell;

const CACHE_ROOT: &str = "/var/lib/dashboard/repo-cache";
const AGENT_HOME: &str = "/home/agent";
const AGENT_REPO: &str = "/home/agent/repo";
const AGENT_BUNDLE: &str = "/tmp/tekton.bundle";
const AGENTS_DIR: &str = "/var/lib/claude-agents";
/// How long the startup scrub waits on one agent before moving on.
const SCRUB_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);
/// Upper bound on prerequisite oids taken from the agent for a thin bundle.
const MAX_PREREQS: usize = 16;

static REPO_LOCKS: LazyLock<DashMap<String, Arc<Mutex<()>>>> = LazyLock::new(DashMap::new);

/// The credential-free remote URL agents get.
pub fn plain_url(repo: &str) -> String {
    format!("https://github.com/{repo}.git")
}

/// Where a repo's objects come from and where the host caches them.
struct Source {
    url: String,
    cache: PathBuf,
}

impl Source {
    fn github(repo: &str) -> Result<Self, AppError> {
        if !valid_repo(repo) {
            return Err(AppError::BadRequest(format!("Invalid repo name '{repo}'")));
        }
        Ok(Source {
            url: plain_url(repo),
            cache: Path::new(CACHE_ROOT).join(format!("{}.git", repo.replace('/', "__"))),
        })
    }
}

fn valid_repo(repo: &str) -> bool {
    let mut parts = repo.split('/');
    let ok_part = |p: &str| {
        !p.is_empty()
            && !p.starts_with('.')
            && p.chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
    };
    matches!((parts.next(), parts.next(), parts.next()), (Some(o), Some(r), None) if ok_part(o) && ok_part(r))
}

/// Branch names end up in refspecs and shell commands; reject anything git or the
/// refspec syntax would treat specially.
fn valid_branch(branch: &str) -> bool {
    !branch.is_empty()
        && !branch.starts_with(['-', '/', '.'])
        && !branch.ends_with(['/', '.'])
        && !branch.ends_with(".lock")
        && !branch.contains("..")
        && !branch.contains("@{")
        && !branch.contains("//")
        && !branch
            .chars()
            .any(|c| c.is_whitespace() || c.is_control() || ":~^?*[\\'\"".contains(c))
}

fn is_oid(s: &str) -> bool {
    matches!(s.len(), 40 | 64) && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// Single-quote a string for `sh`.
fn sq(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// Env vars that make git send the token as an HTTP header for this one process.
fn auth_env(token: &str) -> [(&'static str, String); 3] {
    let basic = base64::engine::general_purpose::STANDARD.encode(format!("x-access-token:{token}"));
    [
        ("GIT_CONFIG_COUNT", "1".to_string()),
        ("GIT_CONFIG_KEY_0", "http.extraHeader".to_string()),
        (
            "GIT_CONFIG_VALUE_0",
            format!("Authorization: Basic {basic}"),
        ),
    ]
}

/// Run git on the host against a bare cache repo.
async fn git(cache: &Path, args: &[&str], token: Option<&str>) -> Result<String, AppError> {
    git_with_stdin(cache, args, token, None).await
}

async fn git_with_stdin(
    cache: &Path,
    args: &[&str],
    token: Option<&str>,
    stdin: Option<&str>,
) -> Result<String, AppError> {
    use tokio::io::AsyncWriteExt;

    tracing::info!(
        "Running: git --git-dir {} {}",
        cache.display(),
        args.join(" ")
    );
    let mut cmd = Command::new("git");
    cmd.arg("--git-dir")
        .arg(cache)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0");
    if let Some(token) = token {
        cmd.envs(auth_env(token));
    }
    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| AppError::Internal(format!("Failed to execute git: {e}")))?;
    if let Some(mut pipe) = child.stdin.take() {
        if let Some(input) = stdin {
            pipe.write_all(input.as_bytes())
                .await
                .map_err(|e| AppError::Internal(format!("Failed to write to git: {e}")))?;
        }
    }
    let output = child
        .wait_with_output()
        .await
        .map_err(|e| AppError::Internal(format!("Failed to wait on git: {e}")))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(AppError::Internal(format!(
            "git {} failed: {stderr}",
            args.first().unwrap_or(&"")
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// Fetch `branches` (and every tag) from `url` into the bare cache repo, creating it if
/// needed. Tags are force-updated so a moved tag upstream never fails the fetch.
async fn fetch_to_cache(
    cache: &Path,
    url: &str,
    token: &str,
    branches: &[&str],
) -> Result<(), AppError> {
    if let Some(b) = branches.iter().find(|b| !valid_branch(b)) {
        return Err(AppError::BadRequest(format!("Invalid branch name '{b}'")));
    }
    if !cache.exists() {
        if let Some(parent) = cache.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| AppError::Internal(format!("Cannot create repo cache dir: {e}")))?;
        }
        git(cache, &["init", "--quiet", "--bare"], None).await?;
    }
    let refspecs: Vec<String> = branches
        .iter()
        .map(|b| format!("+refs/heads/{b}:refs/heads/{b}"))
        .collect();
    let mut args = vec!["fetch", "--quiet", "--no-tags", url];
    args.extend(refspecs.iter().map(String::as_str));
    args.push("+refs/tags/*:refs/tags/*");
    git(cache, &args, Some(token)).await?;
    Ok(())
}

async fn branch_oid(cache: &Path, branch: &str) -> Result<String, AppError> {
    Ok(git(
        cache,
        &[
            "rev-parse",
            "--verify",
            &format!("refs/heads/{branch}^{{commit}}"),
        ],
        None,
    )
    .await?
    .trim()
    .to_string())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BundleKind {
    /// Every branch with its full history.
    Full,
    /// Only the commits the agent's prerequisites don't already cover.
    Thin,
    /// The agent already has every tip; no bundle is needed, only ref updates.
    UpToDate,
}

/// Bundle `branches` from the cache into `out`. Prerequisites (oids the agent says it has)
/// that the cache knows make the bundle thin; none known means a full bundle, which also
/// carries the tags reachable from the branches when `with_tags` is set.
async fn build_bundle(
    cache: &Path,
    branches: &[&str],
    prereqs: &[String],
    with_tags: bool,
    out: &Path,
) -> Result<BundleKind, AppError> {
    let mut known = Vec::new();
    for oid in prereqs.iter().filter(|o| is_oid(o)).take(MAX_PREREQS) {
        if !known.contains(oid)
            && git(
                cache,
                &["cat-file", "-e", &format!("{oid}^{{commit}}")],
                None,
            )
            .await
            .is_ok()
        {
            known.push(oid.clone());
        }
    }
    let heads: Vec<String> = branches.iter().map(|b| format!("refs/heads/{b}")).collect();
    let excludes: Vec<String> = known.iter().map(|o| format!("^{o}")).collect();

    if !known.is_empty() {
        let mut args = vec!["rev-list", "--count"];
        args.extend(heads.iter().map(String::as_str));
        args.extend(excludes.iter().map(String::as_str));
        if git(cache, &args, None).await?.trim() == "0" {
            return Ok(BundleKind::UpToDate);
        }
    }

    let mut revs: Vec<String> = heads.iter().chain(&excludes).cloned().collect();
    if with_tags && known.is_empty() {
        for head in &heads {
            let merged = format!("--merged={head}");
            let tags = git(
                cache,
                &["for-each-ref", "--format=%(refname)", &merged, "refs/tags/"],
                None,
            )
            .await?;
            revs.extend(tags.lines().map(String::from));
        }
        revs.sort();
        revs.dedup();
    }
    // Revisions go through stdin: a repo can have more tags than fit in argv.
    let out = out.to_string_lossy();
    git_with_stdin(
        cache,
        &["bundle", "create", "--quiet", out.as_ref(), "--stdin"],
        None,
        Some(&(revs.join("\n") + "\n")),
    )
    .await?;
    Ok(if known.is_empty() {
        BundleKind::Full
    } else {
        BundleKind::Thin
    })
}

// ── Agent-side commands. None of these take a token, so none can leak one. ──

/// Clone `branch` from the bundle into `<home>/repo`, delete the bundle, and point origin
/// at the plain URL. Leaves the shell in the new repo so callers can append `&& ...`.
fn clone_cmd_at(home: &str, bundle: &str, url: &str, branch: &str) -> String {
    format!(
        "cd {home} && git clone -q --branch {b} {bundle} repo; rc=$?; rm -f {bundle}; \
         test $rc -eq 0 && cd repo && git remote set-url origin {url}",
        home = sq(home),
        bundle = sq(bundle),
        b = sq(branch),
        url = sq(url),
    )
}

/// Load the bundle's objects (if any) and point `origin/<branch>` at each tip.
fn update_refs_cmd_at(repo_dir: &str, bundle: Option<&str>, tips: &[(&str, &str)]) -> String {
    let updates = tips
        .iter()
        .map(|(b, oid)| {
            format!(
                "git update-ref {} {oid}",
                sq(&format!("refs/remotes/origin/{b}"))
            )
        })
        .collect::<Vec<_>>()
        .join(" && ");
    match bundle {
        Some(bundle) => {
            let heads = tips
                .iter()
                .map(|(b, _)| sq(&format!("refs/heads/{b}")))
                .collect::<Vec<_>>()
                .join(" ");
            format!(
                "cd {repo} && git fetch -q {bundle} {heads}; rc=$?; rm -f {bundle}; \
                 test $rc -eq 0 && {updates}",
                repo = sq(repo_dir),
                bundle = sq(bundle),
            )
        }
        None => format!("cd {} && {updates}", sq(repo_dir)),
    }
}

/// Print the commits the agent repo has that a thin bundle can build on.
fn known_oids_cmd_at(repo_dir: &str) -> String {
    format!(
        "cd {} && {{ git rev-parse HEAD; git for-each-ref --format='%(objectname)' refs/remotes/origin/; }} 2>/dev/null",
        sq(repo_dir)
    )
}

/// Strip any credentials from origin's URL. A no-op when there's no repo.
fn scrub_remote_cmd_at(repo_dir: &str) -> String {
    format!(
        "cd {} 2>/dev/null || exit 0; url=$(git remote get-url origin) || exit 0; \
         git remote set-url origin \"$(printf '%s' \"$url\" | sed -E 's#^(https?://)[^@/]*@#\\1#')\"",
        sq(repo_dir)
    )
}

/// Hard-reset the agent's checkout to `origin/<branch>`, e.g. after an API commit.
fn reset_cmd_at(repo_dir: &str, branch: &str) -> String {
    format!(
        "cd {} && git reset -q --hard {}",
        sq(repo_dir),
        sq(&format!("refs/remotes/origin/{branch}"))
    )
}

/// Shell command that resets `/home/agent/repo` to `origin/<branch>`.
pub fn reset_to_origin_cmd(branch: &str) -> String {
    reset_cmd_at(AGENT_REPO, branch)
}

/// Point the agent repo's origin at the plain URL.
pub fn set_plain_remote_cmd(repo: &str) -> String {
    format!(
        "cd {} && git remote set-url origin {}",
        sq(AGENT_REPO),
        sq(&plain_url(repo))
    )
}

/// Shell command that clones `branch` from the staged bundle into `/home/agent/repo` and
/// leaves the shell there. Run [`stage_clone_bundle`] first.
pub fn clone_cmd(repo: &str, branch: &str) -> String {
    clone_cmd_at(AGENT_HOME, AGENT_BUNDLE, &plain_url(repo), branch)
}

// ── Orchestration ──

/// How the host reaches an agent; tests substitute a local directory.
trait Agent {
    fn home(&self) -> &str;
    fn bundle_path(&self) -> &str;
    async fn exec(&self, cmd: &str) -> Result<String, AppError>;
    async fn upload(&self, local: &Path) -> Result<(), AppError>;

    fn repo_dir(&self) -> String {
        format!("{}/repo", self.home())
    }
}

struct SshAgent<'a>(&'a str);

impl Agent for SshAgent<'_> {
    fn home(&self) -> &str {
        AGENT_HOME
    }
    fn bundle_path(&self) -> &str {
        AGENT_BUNDLE
    }
    async fn exec(&self, cmd: &str) -> Result<String, AppError> {
        shell::agent_exec_capture(self.0, cmd).await
    }
    async fn upload(&self, local: &Path) -> Result<(), AppError> {
        shell::scp_to_agent(self.0, &local.to_string_lossy(), AGENT_BUNDLE).await
    }
}

/// Fetch `branches` into the cache and bundle them against `prereqs`. Returns each branch's
/// tip, the bundle kind and (unless up to date) the local bundle file.
async fn prepare(
    src: &Source,
    token: &str,
    branches: &[&str],
    prereqs: &[String],
    with_tags: bool,
) -> Result<(Vec<String>, BundleKind, Option<PathBuf>), AppError> {
    let lock = REPO_LOCKS
        .entry(src.cache.to_string_lossy().into_owned())
        .or_default()
        .clone();
    let _guard = lock.lock().await;

    fetch_to_cache(&src.cache, &src.url, token, branches).await?;
    let mut tips = Vec::new();
    for b in branches {
        tips.push(branch_oid(&src.cache, b).await?);
    }
    let out = src
        .cache
        .with_extension(format!("{}.bundle", uuid::Uuid::new_v4().simple()));
    let kind = build_bundle(&src.cache, branches, prereqs, with_tags, &out).await?;
    let file = (kind != BundleKind::UpToDate).then_some(out);
    Ok((tips, kind, file))
}

/// Copy the bundle into the agent, removing the host copy either way.
async fn upload_bundle(agent: &impl Agent, file: &Path) -> Result<(), AppError> {
    let result = agent.upload(file).await;
    let _ = std::fs::remove_file(file);
    result
}

/// Stage a full bundle of `branch` (plus `extra`, best effort: it may have been deleted
/// upstream, e.g. a merged base branch) with its tags.
async fn stage_clone_bundle_in(
    src: &Source,
    token: &str,
    agent: &impl Agent,
    branch: &str,
    extra: Option<&str>,
) -> Result<(), AppError> {
    let prepared = match extra.filter(|e| *e != branch) {
        Some(extra) => match prepare(src, token, &[branch, extra], &[], true).await {
            Ok(prepared) => prepared,
            Err(e) => {
                tracing::warn!("Could not fetch '{extra}' for clone, continuing without it: {e}");
                prepare(src, token, &[branch], &[], true).await?
            }
        },
        None => prepare(src, token, &[branch], &[], true).await?,
    };
    let file = prepared
        .2
        .ok_or_else(|| AppError::Internal("Clone bundle was not created".into()))?;
    upload_bundle(agent, &file).await
}

/// Bring `origin/<branch>` in the agent repo up to date with the remote and return its tip.
/// Tries a thin bundle (or a bare ref update) first and falls back to a full bundle when the
/// agent turns out to lack the prerequisites.
async fn sync_branch_in(
    src: &Source,
    token: &str,
    agent: &impl Agent,
    branch: &str,
) -> Result<(String, BundleKind), AppError> {
    let repo_dir = agent.repo_dir();
    let prereqs: Vec<String> = agent
        .exec(&known_oids_cmd_at(&repo_dir))
        .await
        .map(|out| out.lines().map(|l| l.trim().to_string()).collect())
        .unwrap_or_default();

    let mut prereqs = prereqs;
    loop {
        let (tips, kind, file) = prepare(src, token, &[branch], &prereqs, false).await?;
        let bundle = match &file {
            Some(file) => {
                upload_bundle(agent, file).await?;
                Some(agent.bundle_path())
            }
            None => None,
        };
        let cmd = update_refs_cmd_at(&repo_dir, bundle, &[(branch, &tips[0])]);
        match agent.exec(&cmd).await {
            Ok(_) => return Ok((tips[0].clone(), kind)),
            Err(e) if kind != BundleKind::Full => {
                tracing::warn!("Thin sync of '{branch}' failed, retrying with a full bundle: {e}");
                prereqs.clear();
            }
            Err(e) => return Err(e),
        }
    }
}

/// Fetch `branch` (and `extra`, if it still exists) on the host and copy a full bundle of
/// them into the agent, ready for [`clone_cmd`].
pub async fn stage_clone_bundle(
    agent_name: &str,
    repo: &str,
    token: &str,
    branch: &str,
    extra: Option<&str>,
) -> Result<(), AppError> {
    stage_clone_bundle_in(
        &Source::github(repo)?,
        token,
        &SshAgent(agent_name),
        branch,
        extra,
    )
    .await
}

/// Update `origin/<branch>` in the agent repo from GitHub and return the branch's head.
pub async fn sync_branch(
    agent_name: &str,
    repo: &str,
    token: &str,
    branch: &str,
) -> Result<String, AppError> {
    let (tip, _) =
        sync_branch_in(&Source::github(repo)?, token, &SshAgent(agent_name), branch).await?;
    Ok(tip)
}

/// One-time cleanup for containers created before tokens left the agents: strip credentials
/// from every live agent's origin URL. Best effort.
pub async fn scrub_agent_remotes() {
    let entries = match std::fs::read_dir(AGENTS_DIR) {
        Ok(entries) => entries,
        Err(e) => {
            tracing::warn!("Cannot list {AGENTS_DIR} to scrub agent remotes: {e}");
            return;
        }
    };
    let cmd = scrub_remote_cmd_at(AGENT_REPO);
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        let scrub = shell::agent_exec_capture(&name, &cmd);
        match tokio::time::timeout(SCRUB_TIMEOUT, scrub).await {
            Ok(Ok(_)) => tracing::info!("Scrubbed git remote in agent '{name}'"),
            Ok(Err(e)) => tracing::warn!("Failed to scrub git remote in agent '{name}': {e}"),
            Err(_) => tracing::warn!("Timed out scrubbing git remote in agent '{name}'"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command as StdCommand;

    const TOKEN: &str = "test-secret-token";

    /// Keeps the developer's global/system git config (e.g. commit signing)
    /// out of test repos.
    fn isolated(cmd: &mut StdCommand) -> &mut StdCommand {
        cmd.env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
    }

    struct LocalAgent {
        home: String,
        bundle: String,
        /// Overrides what the agent reports as the commits it has.
        fake_oids: Option<String>,
    }

    impl Agent for LocalAgent {
        fn home(&self) -> &str {
            &self.home
        }
        fn bundle_path(&self) -> &str {
            &self.bundle
        }
        async fn exec(&self, cmd: &str) -> Result<String, AppError> {
            if let Some(fake) = &self.fake_oids {
                if cmd == known_oids_cmd_at(&self.repo_dir()) {
                    return Ok(fake.clone());
                }
            }
            let out = isolated(StdCommand::new("sh").arg("-c").arg(cmd))
                .output()
                .unwrap();
            if !out.status.success() {
                return Err(AppError::Internal(format!(
                    "sh failed: {}",
                    String::from_utf8_lossy(&out.stderr)
                )));
            }
            Ok(String::from_utf8_lossy(&out.stdout).to_string())
        }
        async fn upload(&self, local: &Path) -> Result<(), AppError> {
            std::fs::copy(local, &self.bundle).unwrap();
            Ok(())
        }
    }

    fn run(dir: &Path, args: &[&str]) -> String {
        let out = isolated(&mut StdCommand::new("git"))
            .args([
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@t",
                "-c",
                "init.defaultBranch=main",
            ])
            .args(args)
            .current_dir(dir)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    /// A stand-in for GitHub: a bare repo plus a work clone to push commits from.
    struct Fixture {
        dir: tempfile::TempDir,
        src: Source,
        agent: LocalAgent,
    }

    impl Fixture {
        fn new() -> Self {
            let dir = tempfile::tempdir().unwrap();
            let p = dir.path();
            run(p, &["init", "-q", "--bare", "upstream.git"]);
            run(p, &["clone", "-q", "upstream.git", "work"]);
            std::fs::create_dir(p.join("home")).unwrap();
            let src = Source {
                url: format!("file://{}", p.join("upstream.git").display()),
                cache: p.join("cache").join("o__r.git"),
            };
            let agent = LocalAgent {
                home: p.join("home").display().to_string(),
                bundle: p.join("tekton.bundle").display().to_string(),
                fake_oids: None,
            };
            let f = Fixture { dir, src, agent };
            f.commit("main", "README", "one");
            f
        }

        fn work(&self) -> PathBuf {
            self.dir.path().join("work")
        }

        fn repo(&self) -> PathBuf {
            self.dir.path().join("home/repo")
        }

        /// Commit a file on `branch` upstream and return the new oid.
        fn commit(&self, branch: &str, file: &str, content: &str) -> String {
            let w = self.work();
            let exists = isolated(&mut StdCommand::new("git"))
                .args([
                    "rev-parse",
                    "--verify",
                    "-q",
                    &format!("refs/heads/{branch}"),
                ])
                .current_dir(&w)
                .output()
                .unwrap()
                .status
                .success();
            if exists {
                run(&w, &["checkout", "-q", branch]);
            } else {
                run(&w, &["checkout", "-q", "-B", branch]);
            }
            std::fs::write(w.join(file), content).unwrap();
            run(&w, &["add", "-A"]);
            run(&w, &["commit", "-q", "-m", content]);
            run(&w, &["push", "-q", "-f", "origin", branch]);
            run(&w, &["rev-parse", "HEAD"])
        }

        async fn clone(&self, checkout: &str, extra: Option<&str>) {
            stage_clone_bundle_in(&self.src, TOKEN, &self.agent, checkout, extra)
                .await
                .unwrap();
            let cmd = clone_cmd_at(
                &self.agent.home,
                &self.agent.bundle,
                "https://github.com/o/r.git",
                checkout,
            );
            self.agent.exec(&cmd).await.unwrap();
        }

        fn assert_no_token_on_disk(&self) {
            let out = StdCommand::new("grep")
                .args(["-r", "-l", TOKEN])
                .arg(self.dir.path())
                .output()
                .unwrap();
            assert!(out.stdout.is_empty(), "token found on disk");
            assert!(
                !Path::new(&self.agent.bundle).exists(),
                "bundle left behind"
            );
        }
    }

    #[tokio::test]
    async fn initial_clone() {
        let f = Fixture::new();
        let base = run(&f.work(), &["rev-parse", "main"]);
        f.clone("main", None).await;
        let repo = f.repo();
        run(&repo, &["checkout", "-q", "-b", "feature"]);
        assert_eq!(run(&repo, &["rev-parse", "origin/main"]), base);
        assert_eq!(
            run(&repo, &["rev-parse", "--abbrev-ref", "HEAD"]),
            "feature"
        );
        assert_eq!(
            run(&repo, &["remote", "get-url", "origin"]),
            "https://github.com/o/r.git"
        );
        f.assert_no_token_on_disk();
    }

    #[tokio::test]
    async fn reopen_clone_has_base_and_task_branch() {
        let f = Fixture::new();
        let base = run(&f.work(), &["rev-parse", "main"]);
        let task = f.commit("task", "a.txt", "task work");
        f.clone("task", Some("main")).await;
        let repo = f.repo();
        assert_eq!(run(&repo, &["rev-parse", "HEAD"]), task);
        assert_eq!(run(&repo, &["rev-parse", "--abbrev-ref", "HEAD"]), "task");
        assert_eq!(run(&repo, &["rev-parse", "origin/main"]), base);
        f.assert_no_token_on_disk();
    }

    #[tokio::test]
    async fn reopen_clone_survives_deleted_base_branch() {
        let f = Fixture::new();
        let task = f.commit("task", "a.txt", "task work");
        f.clone("task", Some("gone")).await;
        let repo = f.repo();
        assert_eq!(run(&repo, &["rev-parse", "HEAD"]), task);
        assert_eq!(run(&repo, &["rev-parse", "--abbrev-ref", "HEAD"]), "task");
        f.assert_no_token_on_disk();
    }

    #[tokio::test]
    async fn clone_carries_reachable_tags() {
        let f = Fixture::new();
        run(&f.work(), &["tag", "-a", "v1.0", "-m", "release"]);
        run(&f.work(), &["tag", "light"]);
        f.commit("other", "x.txt", "unrelated");
        run(&f.work(), &["tag", "on-other"]);
        run(&f.work(), &["push", "-q", "origin", "--tags"]);

        f.clone("main", None).await;
        let repo = f.repo();
        assert_eq!(run(&repo, &["tag", "--list"]), "light\nv1.0");
        assert_eq!(run(&repo, &["describe"]), "v1.0");
        f.assert_no_token_on_disk();
    }

    #[tokio::test]
    async fn thin_bundle_then_up_to_date() {
        let f = Fixture::new();
        f.clone("main", None).await;
        let new = f.commit("main", "b.txt", "two");

        let (tip, kind) = sync_branch_in(&f.src, TOKEN, &f.agent, "main")
            .await
            .unwrap();
        assert_eq!((tip.as_str(), kind), (new.as_str(), BundleKind::Thin));
        assert_eq!(run(&f.repo(), &["rev-parse", "origin/main"]), new);

        // A local commit on top of an unchanged remote needs no bundle at all.
        std::fs::write(f.repo().join("c.txt"), "local").unwrap();
        run(&f.repo(), &["add", "-A"]);
        run(&f.repo(), &["commit", "-q", "-m", "local"]);
        let (tip, kind) = sync_branch_in(&f.src, TOKEN, &f.agent, "main")
            .await
            .unwrap();
        assert_eq!((tip.as_str(), kind), (new.as_str(), BundleKind::UpToDate));
        f.assert_no_token_on_disk();
    }

    #[tokio::test]
    async fn thin_bundle_falls_back_to_full_when_prerequisite_missing() {
        let mut f = Fixture::new();
        f.clone("main", None).await;
        // The cache learns `missing`; the agent never gets it but claims to have it.
        let missing = f.commit("main", "b.txt", "two");
        fetch_to_cache(&f.src.cache, &f.src.url, TOKEN, &["main"])
            .await
            .unwrap();
        let new = f.commit("main", "c.txt", "three");
        f.agent.fake_oids = Some(format!("ssh warning\n{missing}\n"));

        let (tip, kind) = sync_branch_in(&f.src, TOKEN, &f.agent, "main")
            .await
            .unwrap();
        assert_eq!((tip.as_str(), kind), (new.as_str(), BundleKind::Full));
        assert_eq!(run(&f.repo(), &["rev-parse", "origin/main"]), new);
        f.assert_no_token_on_disk();
    }

    #[tokio::test]
    async fn missing_remote_branch_is_an_error() {
        let f = Fixture::new();
        f.clone("main", None).await;
        assert!(sync_branch_in(&f.src, TOKEN, &f.agent, "nope")
            .await
            .is_err());
    }

    #[tokio::test]
    async fn reset_after_api_push() {
        let f = Fixture::new();
        f.clone("main", None).await;
        let repo = f.repo();
        run(&repo, &["checkout", "-q", "-b", "task"]);
        run(&f.work(), &["push", "-q", "origin", "main:task"]);
        std::fs::write(repo.join("feature.txt"), "agent change").unwrap();
        run(&repo, &["add", "-A"]);
        run(&repo, &["commit", "-q", "-m", "agent commit"]);
        // The API creates its own commit with the same content directly upstream.
        let api = f.commit("task", "feature.txt", "agent change");

        let (tip, _) = sync_branch_in(&f.src, TOKEN, &f.agent, "task")
            .await
            .unwrap();
        assert_eq!(tip, api);
        f.agent
            .exec(&reset_cmd_at(&f.agent.repo_dir(), "task"))
            .await
            .unwrap();
        assert_eq!(run(&repo, &["rev-parse", "HEAD"]), api);
        assert_eq!(run(&repo, &["status", "--porcelain"]), "");
        f.assert_no_token_on_disk();
    }

    #[tokio::test]
    async fn scrub_strips_credentials_from_origin() {
        let f = Fixture::new();
        f.clone("main", None).await;
        let authed = ["https://x-access", "-token:abc@github.com/o/r.git"].concat();
        run(&f.repo(), &["remote", "set-url", "origin", &authed]);
        f.agent
            .exec(&scrub_remote_cmd_at(&f.agent.repo_dir()))
            .await
            .unwrap();
        assert_eq!(
            run(&f.repo(), &["remote", "get-url", "origin"]),
            "https://github.com/o/r.git"
        );
        // No repo: nothing to do, not an error.
        f.agent
            .exec(&scrub_remote_cmd_at(
                &f.dir.path().join("none").display().to_string(),
            ))
            .await
            .unwrap();
    }

    #[test]
    fn auth_env_carries_token_only_in_values() {
        let env = auth_env("tok");
        assert_eq!(env[0], ("GIT_CONFIG_COUNT", "1".to_string()));
        assert_eq!(env[1].1, "http.extraHeader");
        let encoded = base64::engine::general_purpose::STANDARD.encode("x-access-token:tok");
        assert_eq!(env[2].1, format!("Authorization: Basic {encoded}"));
    }

    #[test]
    fn validation() {
        assert!(valid_repo("lambdaclass/tekton"));
        assert!(valid_repo("o/r.js"));
        for bad in ["o", "o/r/x", "../r", "o/..", "o/.git", "o/r x", ""] {
            assert!(!valid_repo(bad), "{bad}");
        }
        assert!(valid_branch("main"));
        assert!(valid_branch("feature/fix-1"));
        for bad in [
            "", "-x", "a:b", "a..b", "a b", "a'b", "x.lock", "a/", "a~1", "a^",
        ] {
            assert!(!valid_branch(bad), "{bad}");
        }
        assert!(is_oid(&"a".repeat(40)));
        assert!(!is_oid(&"A".repeat(40)));
        assert!(!is_oid("HEAD"));
    }

    #[test]
    fn agent_commands_quote_arguments() {
        assert_eq!(
            reset_to_origin_cmd("main"),
            "cd '/home/agent/repo' && git reset -q --hard 'refs/remotes/origin/main'"
        );
        assert_eq!(
            set_plain_remote_cmd("o/r"),
            "cd '/home/agent/repo' && git remote set-url origin 'https://github.com/o/r.git'"
        );
        assert!(clone_cmd("o/r", "it's").contains("'it'\\''s'"));
        assert_eq!(
            update_refs_cmd_at("/r", None, &[("main", "abc")]),
            "cd '/r' && git update-ref 'refs/remotes/origin/main' abc"
        );
    }

    /// Tokens may only meet git in this module.
    #[test]
    fn credentials_only_in_repo_sync() {
        let needles = [
            ["x-access", "-token"].concat(),
            ["extra", "Header"].concat(),
        ];
        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut checked = 0;
        for entry in std::fs::read_dir(&src).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_none_or(|e| e != "rs") {
                continue;
            }
            let text = std::fs::read_to_string(&path).unwrap();
            let is_repo_sync = path.file_name().unwrap() == "repo_sync.rs";
            for needle in &needles {
                assert_eq!(
                    text.contains(needle.as_str()),
                    is_repo_sync,
                    "{needle} in {}",
                    path.display()
                );
            }
            checked += 1;
        }
        assert!(checked > 10);
    }
}
