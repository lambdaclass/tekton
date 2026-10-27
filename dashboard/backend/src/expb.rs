//! EXPB benchmark driver — SSH edition.
//!
//! Given a benchmark server (hostname + SSH user + key) and a path to a local
//! ethrex clone on that server, we:
//!
//! 1. SSH in, `git fetch + git checkout <commit>` in the ethrex repo.
//! 2. `docker build -t ethrex:local <ethrex_repo>` to rebuild the image.
//! 3. Generate a YAML scenario config for the selected tier.
//! 4. Run `./scripts/expb-wrapper.sh execute-scenario --scenario-name <name>
//!    --config-file <tmp.yaml> --per-payload-metrics` over SSH.
//! 5. Read the resulting `ethrex.log` and `k6.log` back over SSH and parse
//!    throughput / latency out of them.
//!
//! Per experiment we run the three scenarios in order: `fast → gigablocks →
//! slow`. A tier is compared against that tier's baseline and rejected on the
//! first failure. An experiment is KEPT only if it passes all three tiers.

use std::time::Duration;

use regex::Regex;
use tokio::process::Command;

use crate::error::AppError;
use crate::models::BenchmarkServer;

/// Minimum primary-metric improvement to count as a win (percentage).
const KEEP_IMPROVEMENT_PCT: f64 = 5.0;
/// Max allowed regression on the *other* primary metric (percentage).
const MAX_REGRESSION_PCT: f64 = 5.0;
/// Max allowed regression on any tail latency percentile.
const MAX_TAIL_REGRESSION_PCT: f64 = 10.0;

/// Parsed metrics from one EXPB scenario run.
#[derive(Debug, Clone, Default)]
pub struct ExpbMetrics {
    pub mgas_avg: Option<f64>,
    pub latency_avg_ms: Option<f64>,
    pub latency_p50_ms: Option<f64>,
    pub latency_p95_ms: Option<f64>,
    pub latency_p99_ms: Option<f64>,
}

/// Delta (percentage, signed) between a baseline and an experiment. Positive
/// means the experiment's value is larger than the baseline's in the metric's
/// natural units — good for throughput, bad for latency.
#[derive(Debug, Clone, Default)]
pub struct ExpbComparison {
    pub mgas_delta_pct: Option<f64>,
    pub latency_delta_pct: Option<f64>,
    pub p50_delta_pct: Option<f64>,
    pub p95_delta_pct: Option<f64>,
    pub p99_delta_pct: Option<f64>,
}

impl ExpbComparison {
    /// Compute the delta between a baseline and an experiment in percentage
    /// terms: `(experiment - baseline) / baseline * 100`.
    pub fn from_metrics(baseline: &ExpbMetrics, experiment: &ExpbMetrics) -> Self {
        Self {
            mgas_delta_pct: pct_delta(baseline.mgas_avg, experiment.mgas_avg),
            latency_delta_pct: pct_delta(baseline.latency_avg_ms, experiment.latency_avg_ms),
            p50_delta_pct: pct_delta(baseline.latency_p50_ms, experiment.latency_p50_ms),
            p95_delta_pct: pct_delta(baseline.latency_p95_ms, experiment.latency_p95_ms),
            p99_delta_pct: pct_delta(baseline.latency_p99_ms, experiment.latency_p99_ms),
        }
    }
}

fn pct_delta(baseline: Option<f64>, experiment: Option<f64>) -> Option<f64> {
    match (baseline, experiment) {
        (Some(b), Some(e)) if b.abs() > f64::EPSILON => Some((e - b) / b * 100.0),
        _ => None,
    }
}

/// Which tier an experiment reached before being killed or passing everything.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tier {
    Fast,
    Gigablocks,
    Slow,
}

impl Tier {
    pub fn all() -> [Tier; 3] {
        [Tier::Fast, Tier::Gigablocks, Tier::Slow]
    }

    /// Name as used in the generated YAML (`web-<tier>`) and in file paths.
    pub fn scenario_name(&self) -> &'static str {
        match self {
            Tier::Fast => "web-fast",
            Tier::Gigablocks => "web-gigablocks",
            Tier::Slow => "web-slow",
        }
    }

    /// Short tier label used in the UI and the per-tier baseline JSON keys.
    pub fn short_name(&self) -> &'static str {
        match self {
            Tier::Fast => "fast",
            Tier::Gigablocks => "gigablocks",
            Tier::Slow => "slow",
        }
    }

    /// EXPB scenario knobs. Copied from the upstream benchmarks driver's
    /// config so we generate the same YAML it would.
    fn scenario_config(&self) -> ScenarioConfig {
        match self {
            Tier::Fast => ScenarioConfig {
                payloads_file: "payloads-10k.jsonl",
                fcus_file: "fcus-10k.jsonl",
                delay: 1.0,
                warmup_delay: 1.0,
                warmup: 0,
                amount: 200,
                output_dir: "fast",
            },
            Tier::Gigablocks => ScenarioConfig {
                payloads_file: "payloads-c100.jsonl",
                fcus_file: "fcus-c100.jsonl",
                delay: 0.0,
                warmup_delay: 0.0,
                warmup: 4831,
                amount: 100,
                output_dir: "gigablocks",
            },
            Tier::Slow => ScenarioConfig {
                payloads_file: "payloads-10k.jsonl",
                fcus_file: "fcus-10k.jsonl",
                delay: 0.333,
                warmup_delay: 0.333,
                warmup: 200,
                amount: 5000,
                output_dir: "slow",
            },
        }
    }
}

struct ScenarioConfig {
    payloads_file: &'static str,
    fcus_file: &'static str,
    delay: f64,
    warmup_delay: f64,
    warmup: u32,
    amount: u32,
    output_dir: &'static str,
}

/// Outcome of running an experiment through the full tiered gate.
pub struct TieredResult {
    /// Highest tier the experiment passed. `None` if it failed at `fast`.
    pub tier_reached: Option<Tier>,
    /// Metrics from the highest tier that ran to completion (not necessarily
    /// the one it passed — e.g. if `gigablocks` ran but failed the keep rule,
    /// these are the `gigablocks` metrics).
    pub final_metrics: Option<ExpbMetrics>,
    /// True only if the experiment passed all three tiers.
    pub keep: bool,
}

/// Run one scenario end-to-end on a benchmark server over SSH:
/// build the docker image, run the EXPB wrapper, read back the log files,
/// parse metrics.
///
/// `ethrex_repo_path` is the absolute path (or `~/...`) to the ethrex git
/// clone on the benchmark host; `benchmarks_repo_path` is the checkout that
/// holds `scripts/expb-wrapper.sh` and the payload / FCU data files.
pub async fn run_scenario_over_ssh(
    server: &BenchmarkServer,
    ethrex_repo_path: &str,
    benchmarks_repo_path: &str,
    tier: Tier,
) -> Result<ExpbMetrics, AppError> {
    let cfg = tier.scenario_config();
    let scenario_name = tier.scenario_name();

    // Unique tag so parallel runs from different branches can't trample each
    // other's image. (Within one autoresearch run they're serial anyway.)
    let image_tag = format!("ethrex:local-{}", scenario_name);

    // 1. Build the docker image from the ethrex checkout.
    ssh_exec(
        server,
        &format!("docker build -t {image_tag} {ethrex_repo_path}"),
    )
    .await?;

    // 2. Write a scenario config YAML to /tmp on the benchmark host and run
    //    the EXPB wrapper. Everything else (payloads, FCUs, output dir) is
    //    relative to `benchmarks_repo_path` to match the Elixir driver's
    //    conventions.
    let config_yaml = scenario_yaml(tier, &image_tag, benchmarks_repo_path, &cfg);
    let remote_cfg_path = format!("/tmp/tekton-expb-{scenario_name}.yaml");
    let escaped_yaml = shell_escape_heredoc(&config_yaml);
    ssh_exec(
        server,
        &format!("cat > {remote_cfg_path} <<'TEKTON_EOF'\n{escaped_yaml}\nTEKTON_EOF"),
    )
    .await?;

    // 3. Run the benchmark. stderr_to_stdout so failures surface in the same
    //    log stream.
    ssh_exec(
        server,
        &format!(
            "cd {benchmarks_repo_path} && ./scripts/expb-wrapper.sh execute-scenario \
             --scenario-name {scenario_name} \
             --config-file {remote_cfg_path} \
             --per-payload-metrics"
        ),
    )
    .await?;

    // 4. Find the output directory and read the two log files.
    let output_root = format!("{benchmarks_repo_path}/expb_output/{}", cfg.output_dir);
    let latest_output = ssh_exec(
        server,
        &format!("ls -dt {output_root}/expb-executor-{scenario_name}-* 2>/dev/null | head -1"),
    )
    .await?
    .trim()
    .to_string();
    if latest_output.is_empty() {
        return Err(AppError::Internal(format!(
            "EXPB finished but no output directory was found under {output_root}"
        )));
    }

    let ethrex_log = ssh_exec(server, &format!("cat {latest_output}/ethrex.log"))
        .await
        .unwrap_or_default();
    let k6_log = ssh_exec(server, &format!("cat {latest_output}/k6.log"))
        .await
        .unwrap_or_default();

    Ok(ExpbMetrics {
        mgas_avg: parse_mgas_avg(&ethrex_log),
        latency_avg_ms: parse_k6_duration(&k6_log, "avg"),
        latency_p50_ms: parse_k6_duration(&k6_log, "med"),
        latency_p95_ms: parse_k6_duration(&k6_log, "p(95)"),
        latency_p99_ms: parse_k6_duration(&k6_log, "p(99)"),
    })
}

/// Run the fast → gigablocks → slow ladder for a branch that's already
/// checked out in the ethrex repo on the server. `baselines` maps each tier
/// to its baseline metrics (stored on the run during baseline seeding).
pub async fn run_tiered(
    server: &BenchmarkServer,
    ethrex_repo_path: &str,
    benchmarks_repo_path: &str,
    baselines: &[(Tier, ExpbMetrics)],
) -> TieredResult {
    let mut tier_reached: Option<Tier> = None;
    let mut final_metrics: Option<ExpbMetrics> = None;

    for (tier, baseline) in baselines {
        let metrics = match run_scenario_over_ssh(
            server,
            ethrex_repo_path,
            benchmarks_repo_path,
            *tier,
        )
        .await
        {
            Ok(m) => m,
            Err(_) => break,
        };
        final_metrics = Some(metrics.clone());
        let cmp = ExpbComparison::from_metrics(baseline, &metrics);
        if !should_keep(&cmp) {
            break;
        }
        tier_reached = Some(*tier);
    }

    TieredResult {
        keep: tier_reached == Some(Tier::Slow),
        tier_reached,
        final_metrics,
    }
}

/// Keep / discard rule for a single comparison.
///
/// KEEP when:
///   (mgas improved ≥5% OR latency improved ≥5%)
///   AND the other primary metric didn't regress >5%
///   AND no tail percentile (p50, p95, p99) regressed >10%.
pub fn should_keep(cmp: &ExpbComparison) -> bool {
    let mgas_improved = cmp
        .mgas_delta_pct
        .is_some_and(|d| d >= KEEP_IMPROVEMENT_PCT);
    // latency is "smaller = better", so an improvement is a negative delta.
    let latency_improved = cmp
        .latency_delta_pct
        .is_some_and(|d| d <= -KEEP_IMPROVEMENT_PCT);

    if !(mgas_improved || latency_improved) {
        return false;
    }

    let mgas_ok = cmp.mgas_delta_pct.is_none_or(|d| d >= -MAX_REGRESSION_PCT);
    let latency_ok = cmp
        .latency_delta_pct
        .is_none_or(|d| d <= MAX_REGRESSION_PCT);
    if !(mgas_ok && latency_ok) {
        return false;
    }

    for d in [cmp.p50_delta_pct, cmp.p95_delta_pct, cmp.p99_delta_pct]
        .into_iter()
        .flatten()
    {
        if d > MAX_TAIL_REGRESSION_PCT {
            return false;
        }
    }
    true
}

/// Build the scenario YAML we hand to `expb-wrapper.sh`. This mirrors the
/// upstream benchmarks driver's config format.
fn scenario_yaml(
    tier: Tier,
    image_tag: &str,
    benchmarks_repo_path: &str,
    cfg: &ScenarioConfig,
) -> String {
    let ScenarioConfig {
        payloads_file,
        fcus_file,
        delay,
        warmup_delay,
        warmup,
        amount,
        output_dir,
    } = cfg;
    let scenario_name = tier.scenario_name();
    format!(
        "pull_images: false\n\
         k6_image: grafana/k6:1.1.0\n\
         paths:\n  \
           work: {benchmarks_repo_path}/expb_work\n  \
           outputs: {benchmarks_repo_path}/expb_output/{output_dir}\n\
         resources:\n  \
           cpu: 8\n  \
           mem: 64g\n  \
           download_speed: 50mbit\n  \
           upload_speed: 15mbit\n\
         scenarios:\n  \
           {scenario_name}:\n    \
             client: ethrex\n    \
             snapshot_source: /root/.ethrex_db/\n    \
             snapshot_backend: overlay\n    \
             image: {image_tag}\n    \
             payloads: {benchmarks_repo_path}/expb_data/{payloads_file}\n    \
             fcus: {benchmarks_repo_path}/expb_data/{fcus_file}\n    \
             duration: 240m\n    \
             warmup_duration: 240m\n    \
             delay: {delay}\n    \
             warmup_delay: {warmup_delay}\n    \
             warmup: {warmup}\n    \
             amount: {amount}\n"
    )
}

/// Extract the average throughput from `ethrex.log` lines like
/// `[METRIC] BLOCK 22365196 | 0.954 Ggas/s | 7 ms | 137 txs | 7 Mgas (19%)`.
/// We convert Ggas/s → Mgas/s (×1000) to match the UI's unit.
fn parse_mgas_avg(ethrex_log: &str) -> Option<f64> {
    let re = Regex::new(r"\[METRIC\] BLOCK \d+ \| (\d+\.?\d*) Ggas/s").ok()?;
    let values: Vec<f64> = re
        .captures_iter(ethrex_log)
        .filter_map(|c| c.get(1)?.as_str().parse::<f64>().ok())
        .filter(|v| *v > 0.0)
        .collect();
    if values.is_empty() {
        return None;
    }
    let sum: f64 = values.iter().sum();
    Some(sum / values.len() as f64 * 1000.0)
}

/// Parse a k6 `http_req_duration` stat out of `k6.log`, looking at the
/// SCENARIO section's `engine_newPayload` group only (not the setup/warmup
/// section, which would skew results). `which` is the exact k6 token:
/// `"avg"`, `"med"`, `"p(95)"`, `"p(99)"`, etc.
fn parse_k6_duration(k6_log: &str, which: &str) -> Option<f64> {
    // Locate the SCENARIO block: k6 prints a "█ SCENARIO:" line once the
    // warmup is over. Skip everything before it.
    let scenario_start = k6_log.find("█ SCENARIO")?;
    let scenario_tail = &k6_log[scenario_start..];
    // Find the engine_newPayload group within the scenario block.
    let group_start = scenario_tail.find("engine_newPayload")?;
    let group_tail = &scenario_tail[group_start..];
    // Find the http_req_duration line.
    let dur_start = group_tail.find("http_req_duration")?;
    let dur_line_end = group_tail[dur_start..]
        .find('\n')
        .map(|e| dur_start + e)
        .unwrap_or(group_tail.len());
    let line = &group_tail[dur_start..dur_line_end];

    // e.g. "http_req_duration..........: avg=18.99ms min=1.13ms med=1.94ms max=48.85ms p(90)=45.99ms p(95)=47.42ms p(99)=48.57ms"
    //
    // Build a regex specific to the token we want.
    let pattern = format!(r"{}=([0-9.]+)(ms|s|µs|us)", regex::escape(which));
    let re = Regex::new(&pattern).ok()?;
    let caps = re.captures(line)?;
    let value: f64 = caps.get(1)?.as_str().parse().ok()?;
    let unit = caps.get(2)?.as_str();
    Some(match unit {
        "s" => value * 1000.0,
        "ms" => value,
        "µs" | "us" => value / 1000.0,
        _ => value,
    })
}

/// Run a shell command on a benchmark server via SSH, return its stdout.
/// Fails with the server's stderr on any non-zero exit. No timeout — the
/// caller is expected to await for as long as the benchmark takes, since
/// EXPB scenarios can run for half an hour.
async fn ssh_exec(server: &BenchmarkServer, command: &str) -> Result<String, AppError> {
    let mut args: Vec<String> = vec![
        "-o".into(),
        "StrictHostKeyChecking=no".into(),
        "-o".into(),
        "UserKnownHostsFile=/dev/null".into(),
        "-o".into(),
        "LogLevel=ERROR".into(),
    ];
    if let Some(key) = server.ssh_key_path.as_deref() {
        args.push("-i".into());
        args.push(key.to_string());
    }
    args.push(format!("{}@{}", server.ssh_user, server.hostname));
    args.push(command.to_string());

    let output = Command::new("ssh")
        .args(&args)
        .output()
        .await
        .map_err(|e| AppError::Internal(format!("Failed to spawn ssh: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        return Err(AppError::Internal(format!(
            "SSH command failed on {} (exit {:?}): {}\nstdout: {}",
            server.hostname,
            output.status.code(),
            stderr.trim(),
            stdout.trim(),
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// Escape a string so it's safe to embed inside a `<<'EOF'` heredoc on the
/// remote side. The quoted form means no shell expansion happens; we only
/// need to make sure the EOF sentinel never appears inside the payload.
fn shell_escape_heredoc(s: &str) -> String {
    // Our sentinel is `TEKTON_EOF`. Replace it with a tombstone if it ever
    // shows up verbatim.
    s.replace("TEKTON_EOF", "TEKTON_EOF_ESCAPED")
}

/// How long (roughly) each tier is expected to take end-to-end. Informational
/// only — the SSH calls don't time out on a schedule; they wait until the
/// remote command returns. If you want to kill a stuck run, use the Stop
/// button on the autoresearch UI.
#[allow(dead_code)]
pub fn approximate_duration(tier: Tier) -> Duration {
    match tier {
        Tier::Fast => Duration::from_secs(4 * 60),
        Tier::Gigablocks => Duration::from_secs(8 * 60),
        Tier::Slow => Duration::from_secs(30 * 60),
    }
}
