# Tekton

> *tekton* (τέκτων): Greek for "builder"

Tekton is a self-hosted platform for running background AI coding agents at scale. You describe what you want, agents write the code, open the PR, and deploy a preview. Not a chatbot. Infrastructure that builds things for you.

It runs on NixOS with isolated systemd-nspawn containers on bare metal.

Inspired by [Michael Stapelberg's post on running coding agents in NixOS](https://michael.stapelberg.ch/posts/2026-02-01-coding-agent-microvm-nix/).

## Why self-hosted

Most AI coding tools are SaaS. You pay per seat, your code leaves your network, and you're locked into one model. Tekton runs on your own hardware.

- **Your infrastructure.** Bare metal servers you control, not shared cloud VMs.
- **Your API keys.** Bring your own accounts, switch models, no markup.
- **No per-seat pricing.** One server handles your whole team.
- **Full isolation.** Each agent gets its own NixOS container with no access to other tasks.
- **Reproducible.** The entire stack is defined in Nix, from the host OS to every container.

## How it works

```
1. You submit a task            "Add pagination to the users endpoint"
2. Tekton spins up a container  Isolated NixOS environment with the repo cloned (~3s)
3. An AI agent does the work    Writes code, runs tests, iterates on its own
4. Results are delivered        Branch pushed, PR created, live preview deployed
```

The dashboard streams agent logs in real time and lets you send follow-up prompts mid-task.

## What it does today

- **PR preview deployments.** Automatic preview environments for Node.js and Elixir/Phoenix apps via GitHub webhooks.
- **Web dashboard.** Create tasks, watch live logs over WebSocket, send follow-up prompts, view preview screenshots.
- **Voice input and repo auto-detection.** Speak your task, the repo is classified automatically.
- **Subtask spawning.** Agents can split work into parallel child tasks.

## Getting started

### Prerequisites

- Local machine with [Nix installed](https://nixos.org/download/) (flakes enabled)
- SSH key pair (`ssh-keygen` if you don't have one)
- A dedicated server in rescue mode (tested on Hetzner, should work on any provider with standard Linux rescue)

### Setup

```bash
# Provision and configure the server (one command)
./setup.sh

# With Elixir/Phoenix preview support
./setup.sh --vertex
```

The script handles everything: network detection, NixOS installation, server configuration, and Claude login. See the [Deployment Guide](docs/deployment-guide.md) for details.

### Agent management

```bash
# Create an agent (~3 seconds)
ssh root@YOUR_SERVER_IP 'agent create myagent'

# SSH into the agent
ssh -J root@YOUR_SERVER_IP agent@<container-ip>

# Run the coding agent
claude
claude --dangerously-skip-permissions  # headless mode

# List and destroy
ssh root@YOUR_SERVER_IP 'agent list'
ssh root@YOUR_SERVER_IP 'agent destroy myagent'

# Rebuild agent closure after config changes
ssh root@YOUR_SERVER_IP 'agent build'
```

### Preview management

```bash
# Deploy a branch (auto via webhook, or manually)
ssh root@YOUR_SERVER_IP 'preview create owner/repo branch-name'

# Elixir/Phoenix preview
ssh root@YOUR_SERVER_IP 'preview create owner/repo branch --type vertex --slug pr-42'

# Monitor and manage
ssh root@YOUR_SERVER_IP 'preview list'
ssh root@YOUR_SERVER_IP 'preview logs pr-42 --follow'
ssh root@YOUR_SERVER_IP 'preview update pr-42'
ssh root@YOUR_SERVER_IP 'preview destroy pr-42'

# Rebuild preview closures
ssh root@YOUR_SERVER_IP 'preview build'
ssh root@YOUR_SERVER_IP 'preview build --type vertex'
```

See [Preview Deployments](docs/preview-deployments.md) for webhook setup and full reference.

### Host maintenance

```bash
# Rebuild NixOS after config changes
ssh root@YOUR_SERVER_IP 'cd /etc/nixos && nixos-rebuild switch'

# Re-authenticate Claude
ssh root@YOUR_SERVER_IP 'CLAUDE_CONFIG_DIR=/var/secrets/claude claude login'
ssh root@YOUR_SERVER_IP 'chmod -R a+rX /var/secrets/claude'
ssh root@YOUR_SERVER_IP 'agent destroy myagent && agent create myagent'  # pick up new creds
```

### Troubleshooting

| Problem | Solution |
|---------|----------|
| Container fails to start | `journalctl -u container@<name>` on host |
| SSH asks for password | SSH key not baked in. Run `agent build` after config changes, then recreate. |
| SSH host key warning | `ssh-keygen -R <container-ip>` |
| Claude "Invalid API key" | Re-run `claude login` on host, fix permissions, recreate container |
| Preview returns 502 | App is still building. Check `preview logs <slug> --follow`. |
| Webhook not triggering | `systemctl status preview-webhook` and check GitHub webhook deliveries |

## Roadmap

### Roadmap overview

**Now (P0):** Safe-by-default core platform: durable task history, policy controls, and repo intelligence before wide rollout.  
**Next (P1):** Scale and collaboration foundations.  
**Later (P2-P4):** Workflow distribution, integrations, observability, and intelligence.

### Roadmap principles

- **Safety before autonomy.** Every increase in agent autonomy must be paired with explicit policy controls and auditability.
- **Planning before execution.** High-confidence plan generation and approval gates come before broad task automation.
- **Workflow-native adoption with Tekton as control plane.** Tekton dashboard is the primary place for execution, approvals, and governance. GitHub/Slack/Linear/Jira are secondary entry points for task intake, notifications, and status sync.
- **Browser-first execution model.** Tekton is intentionally browser-first for shared visibility, approvals, auditability, and zero local setup; IDE-native convenience is a deliberate tradeoff, not a roadmap miss.
- **Operational clarity.** Queue behavior, capacity, costs, and failure modes must be visible and predictable.
- **Composable standards.** Favor interoperable repo-local configuration (`AGENTS.md`-style) over closed one-off config.

### Competitor Gap Snapshot

| Capability | Competitor signal | Tekton status | Priority | Decision |
|------------|-------------------|---------------|----------|----------|
| IDE-native daily workflow (autocomplete + inline edits + agent in editor) | Continue/Cline/Copilot are strong in-editor | Intentionally not core (browser-first) | N/A | **Do not copy now**: win on shared browser workflows + governance |
| Policy engine (repo/org guardrails, approvals, restricted actions) | Copilot, Codex, Cline emphasize explicit permissioning | Planned (roadmap) | P0 | **Copy + improve**: policy presets + per-repo overrides |
| Fine-grained per-tool auto-approval UX | Cline-style safe-vs-risky command behavior | Not explicit | P0/P1 | **Copy + improve**: explicit policy prompts + risk tiers + audit trail |
| Approval ergonomics (approve with constraints) | Advanced agents support nuanced human gating | Not explicit | P1 | **Improve**: scoped approvals by files/tools/operations |
| Network egress controls (allowlist/denylist) | Enterprise agents push sandbox network controls | Planned (roadmap) | P0 | **Copy**: ship default-safe profiles first |
| Pre-task repo intelligence (`ask` + planning context) | Devin Ask flow, planning-first tools | Planned (roadmap) | P0 | **Improve**: couple analysis + plan approval in one flow |
| Collaborative draft/plan approval workflow | Devin/OpenHands style planning loops | Planned (roadmap) | P0/P1 | **Improve**: comments/approvals tied to execution gates |
| Playbooks from successful runs | Devin advanced workflow + team automation products | Planned (templates/playbooks) | P1 | **Improve**: auto-generate playbooks from completed tasks |
| Batch execution across repos/branches | Common in platform-oriented agent tools | Partial concept | P1/P2 | **Copy + improve**: queue-aware, resumable batch jobs |
| Workflow-native integrations (GitHub/Slack/Linear/Jira) | Copilot/Factory/Codegen focus on in-workflow triggers | GitHub-focused roadmap | P2 | **Improve**: bi-directional state/comment sync |
| Portable repo-level agent config (`AGENTS.md`-style) | Open agent ecosystem converging on repo-scoped config | Planned concept (skills) | P1 | **Copy**: adopt compatible schema + migration path |
| Multi-agent orchestration for one task | OpenHands/research agents and commercial products explore this | Planned (roadmap) | P2 | **Improve**: comparison UI + merge strategy |
| Cost controls and budgets by org/team/user | Standard in team/enterprise agent platforms | Planned (roadmap) | P1 | **Copy**: hard limits + alerts + cost attribution |
| Observability and auditability | Enterprise expectation across platforms | Planned (roadmap) | P1/P2 | **Copy + improve**: audit trail + quality KPIs tied to repos |
| Trajectory inspection + evaluation tooling | SWE-agent and eval-first teams emphasize run trace analysis | Not explicit | P1 | **Copy + improve**: trace viewer + replay + quality metrics |
| Deterministic task replay | Reliability/compliance teams need reproducible runs | Not explicit | P1 | **Improve**: snapshot inputs/runtime/tool versions for exact replay |
| Secrets provenance + leak detection | Enterprise platforms require stronger secret governance | Not explicit | P0/P1 | **Copy + improve**: source tracking + redaction + exfil alerts |
| Multi-runtime ecosystem breadth | OpenHands-style runtime flexibility (local/docker/remote) | Partial (nspawn-centric) | P2/P3 | **Improve**: runtime adapter interface with policy parity |

### Why Tekton Is Useful (Even With Strong Competitors)

The goal is not to beat every coding agent on every axis. The goal is to win for teams that need controllable, self-hosted background agents with predictable operations.

- **Trust and control over autonomy theater.** Teams can adopt AI coding workflows with explicit guardrails (policy, approvals, egress controls) instead of opaque defaults.
- **Vendor independence.** BYO model/provider strategy prevents lock-in and keeps model choice flexible as quality and pricing change.
- **Lower long-term unit economics.** For team usage, self-hosted infrastructure + provider flexibility can reduce cost per merged PR versus seat-priced SaaS.
- **Data locality and compliance fit.** Sensitive repositories and execution paths stay in customer-controlled environments.
- **Standardized execution environments.** Reproducible runtime + policy profiles reduce variance and make outcomes more predictable.
- **Compounding institutional memory.** Plans, logs, outcomes, and playbooks become reusable organizational knowledge.
- **Operational leverage at team scale.** Batch/recurrent/event-driven task execution converts one-off prompting into repeatable automation.
- **Auditability and accountability.** Full action history clarifies who asked what, what ran, what was blocked, and why.
- **Private-system integration upside.** Self-hosted architecture is better suited for internal APIs, tooling, and security controls that SaaS tools cannot access easily.

If Tekton executes on these dimensions, it does not need to out-market incumbents on generic "agent coding UX" to be a strong product.

### Phase Exit Criteria

| Phase | Exit criteria |
|-------|---------------|
| P0 | Persisted task/thread history, policy engine + egress controls, repo intelligence, plan approval flow, multi-model routing, and repo-local agent config support |
| P1 | Elastic fleet management, queue priority/preemption, cost/budget controls, audit log baseline, collaborative threads, deterministic replay, constrained approvals, and playbook generation with batch execution |
| P2 | CI/review auto-iteration to merged state, GitHub App + Slack/Linear/Jira workflow sync, API/SDK maturity, and plugin ecosystem with guarded runtime permissions |
| P3 | Multi-repo/monorepo orchestration, recurring/event-driven tasks, rollback/undo workflows, and review-mode task types |
| P4 | Org-wide quality intelligence, checkpoint/fork workflows, conflict prevention/resolution, and optimization loops from historical outcomes |

### Glossary (short)

- **Task:** A unit of work with prompts, logs, and outputs.
- **Thread:** The conversation attached to a task.
- **Agent:** The runtime inside a container that executes the task.
- **Preview:** A deployed environment for a PR/branch.

### Foundational dependencies

- **Auth + identity.** Users, orgs, teams.
- **Data model + storage.** Tasks, threads, logs, artifacts.
- **Job orchestration.** Queue, scheduling, lifecycle states.
- **Metrics + audit trail.** Usage, costs, and action history.

### P0: Core platform (build next)

**P0 must-haves.**
- **Task and prompt persistence.** Every prompt, response, agent action, and conversation thread stored in a database with full history. Nothing is lost when a container is destroyed. Tasks are queryable and searchable across the org.
- **Security and access control.** RBAC for teams and orgs: who can create tasks, which repos they can target, who can approve plans, who can see costs. Secret management for agent credentials (database passwords, API keys, service tokens) injected at runtime, never stored in prompts or logs.
- **Policy engine and sandbox controls.** Repo and org guardrails for what agents are allowed to do: branch protections, command approval policies, network egress allowlists/denylists, and runtime capability restrictions.
- **Fine-grained tool approval UX.** Per-tool and per-command risk tiers (auto-allow, ask, deny), with clear safe-vs-risky behavior, policy explanations, and full decision logging.
- **Secrets provenance and leak prevention.** Track provenance for every injected secret, enforce redaction in prompts/logs/artifacts, and detect likely exfiltration patterns with alerting.
- **Identity-safe PR authorship.** Support user-token PR creation and pushing (in addition to app identity) so branch protection and approval policies stay compliant with org rules.
- **Multi-model support.** Each user connects their own API accounts (Claude, ChatGPT, Gemini, etc.) and Tekton auto-detects access based on their email. For overflow or users without keys, a shared pool routes through OpenRouter. Configuration is per-org: which models are available, which is the default, and spending limits per user/team.

**Repo intelligence + draft/plan mode.** Before coding, an agent indexes and analyzes the repo, answers architecture questions, and proposes a plan (files to change, approach, tradeoffs) as a rendered Markdown draft. Team members comment, suggest changes, approve or reject. Comments are fed back to the agent, which can regenerate the plan. Only after approval does execution begin. Configurable per task: skip straight to coding for simple fixes, require approval for larger changes.

**Portable agent configuration.** Adopt a standard, repo-local agent config format (for example `AGENTS.md`) that maps to Tekton skills, runtime policies, and tool permissions.

**First demo milestone (definition of done).**
- ✅ Single node install
- ✅ Single repo task creation
- ✅ Agent runs, opens PR
- ✅ Preview deployed

### P1: Scale and collaboration foundations

**Elastic infrastructure.** You configure a base fleet of bare metal machines and Tekton manages them as a pool. When demand exceeds capacity, it provisions cloud VPS instances (AWS, GCP, Hetzner Cloud, OVH Cloud) and runs NixOS VMs inside them. When demand drops, the elastic nodes are torn down. All declarative in a config file: base fleet size, max elastic nodes, provider credentials, and scaling thresholds.

**Prewarmed sandbox image pipeline.** Build and refresh repo runtime images/snapshots on a schedule so task startup is near-instant and predictable under load.

**Queue management and priority.** Priority levels (urgent, normal, background), fair scheduling across users, and preemption. High-priority tasks bump low-priority ones when capacity is constrained. Visibility into queue depth and wait times.

**Cost tracking and budgets.** Track token usage per task, compute time per container, aggregate by user and team. Org admins set spending limits and get alerts. Dashboard shows burn rate, cost per merged PR, and trends over time.

**Audit log baseline.** Every action, every prompt, and every external call tied to actor identity and timestamp for compliance, security, and incident review.

**Trajectory inspection and evaluation tooling.** Persist run trajectories (actions, tool calls, outputs, decisions), provide replay/inspection, and score outcomes across task types. Use this to improve prompts, policies, model routing, and failure recovery.

**Deterministic task replay.** Snapshot task inputs, runtime image/version, model settings, and tool versions so failed or risky runs can be reproduced exactly for debugging and compliance.

**Constrained approval ergonomics.** Allow "approve with constraints" decisions (for example: allowed file paths, denied operations, budget/time ceilings) instead of only binary approve/deny.

**Conversational threads.** Every task has a conversation thread, like GitHub PR discussions. Anyone on the team can jump into a running task, add follow-up prompts, see the full history, and see every preview URL generated along the way. Every message shows who submitted it.

**Playbooks from successful runs.** Convert completed tasks into reusable playbooks automatically (prompt, changed files, validation steps), then run them in batch across repositories or branches.

**Repo classifier fallback UX.** If repo classification confidence is low, return "unknown" with top candidates and require explicit user confirmation instead of silently guessing.

### P2: Workflow distribution and automation

**CI integration and agent self-correction.** After an agent opens a PR, watch CI results. If tests fail, the agent picks up the failure logs and iterates. If a human leaves review comments, the agent addresses them without a new prompt. The loop ends at "PR merged", not "PR created."

**Real-time collaboration.** Everything via WebSockets. When one person sends a follow-up, everyone watching sees it. When the agent writes code, everyone sees the logs stream. Uses Operational Transformation for shared state. Multiple users can be active in the same task simultaneously.

**GitHub App integration.** Comment `/tekton fix this` on an issue and it picks up the task. Comment `/tekton address this feedback` on a PR review and it iterates. Conversational threads sync bidirectionally between Tekton and GitHub. Turns Tekton from a tool you go to into a tool that meets you where you already work.

**Workflow-native integrations.** Extend beyond GitHub to Slack, Linear, and Jira so tasks can be created and updated where teams already work, with bidirectional state and comment sync.

**Unified multi-client session sync.** Treat dashboard, GitHub comments, Slack, and ticket systems as synchronized clients over one canonical task/session state.

**Agent memory.** A per-repo knowledge base that persists across tasks: architecture notes, conventions, past mistakes, debugging insights. When an agent starts a new task on a repo it's seen before, it gets this context automatically. The knowledge base grows with every task.

**Agent tool ecosystem.** An MCP-style plugin system where you register tools agents can use per-repo or per-org: run migrations, query monitoring, check Sentry, call internal APIs. This is what separates a coding agent from an engineering agent.

**Built-in web research tools.** First-class `web_search` and `web_fetch` tools with org/repo allowlisted domains, source citation in trajectory logs, secrets redaction on fetched content, and rate/cost guardrails.

**In-sandbox collaborative editor and verification surface.** Add a hosted editor and live verification stream (terminal/app desktop/screenshot timeline) so humans can inspect and steer agent work inside the same sandbox context.

**Private workspaces.** A team or group of people can create a private workspace tied to a repo or conversation, like a private Google Doc. Only invited members see the tasks, prompts, agent output, and previews. Everyone else sees nothing. For management working on financial modeling, a security team investigating a vulnerability, or a small group prototyping before sharing.

**API and SDK.** REST API and client SDK to create tasks, poll status, get results, and manage agents programmatically. Enables integration into CI pipelines, internal tools, and third-party systems.

**Custom agent runtimes.** The agent runtime should be pluggable. Tekton handles the container, the repo, the PR, and the preview. What runs inside (Claude Code, Aider, OpenHands, Codex, custom scripts) is configurable per task or per repo.

**Skills and prompt configuration.** Store reusable skill files (system prompts, instructions, constraints) in a private repo or in the database. Attach one or more skills when creating a task: "follow our API design guidelines", "use our testing conventions." Skills are composable and scoped per repo, per team, or per task. Encodes institutional knowledge into every agent run.

**Duplicate work detection.** Monitors all active and recent prompts across the org. When someone submits a task that overlaps with something in progress or recently completed, Tekton flags it. Uses embeddings over prompt + repo + file paths for semantic similarity, not keyword matching.

**Multi-agent tasks.** Spin up N agents for a single task, each taking a different approach or working on a different part. Compare results, pick the best one, or merge them. Also useful for model comparison: same prompt, different models, compare output.

**Local model support.** Self-hosted models (Llama, DeepSeek, Qwen, etc.) running on the same infrastructure or a dedicated GPU node. For teams that can't send code to external providers, and for cutting costs on simpler tasks.

**Sandbox network policies.** Configurable network policies per repo or per task: allow package registries (npm, pip, hex), block everything else, or full access. Limits blast radius if a model behaves badly.

### P3: Cross-repo workflows and orchestration

**Approval gates.** Before an agent pushes or creates a PR, optionally pause and present the diff for human review. Configurable per repo or per team. The approval can happen in the dashboard, in Slack, or via a GitHub review.

**Rollback/undo.** One-click revert of everything an agent did: close the PR, delete the branch, destroy the preview, undo the commits. Clean, total undo.

**Notifications and integrations.** Slack, Discord, and email notifications when tasks complete, fail, or need follow-up. Outgoing webhook events (task.completed, preview.ready, task.failed) for custom workflows.

**Task templates and playbooks.** Reusable templates with variable slots: "upgrade dependency X across all repos", "add an endpoint with tests." Repos can ship a `.tekton/playbooks/` directory with predefined tasks that show up in the UI.

**Scheduled and recurring tasks.** Cron-like jobs: "every Monday, check for dependency updates", "after every release, update the changelog." Define them in the dashboard or in a `.tekton/schedules` config.

**Event-driven tasks.** Trigger tasks from external events. A Sentry error creates a fix task. A PagerDuty alert spawns a debugging agent. A Slack message kicks off a task. Tekton reacts to what's happening in production.

**Code review as a task type.** "Review this PR for security issues." "Check if this PR follows our conventions." Agents as reviewers, not just authors. First-pass review before a human looks at it.

**Multi-repo tasks.** A single task that coordinates agents across multiple repos, with each agent aware of what the others are doing and results linked together. For changes that span an API and its client library, or a shared schema and all consumers.

**Monorepo support.** Partial clones, understanding package boundaries, running only affected tests, knowing which team owns which directory.

**Multi-runtime support.** Keep nspawn as the default, then add a runtime adapter layer for Docker/Kubernetes/VM-backed sandboxes while preserving the same policy, audit, and queue semantics across runtimes.

**Image and log attachments in prompts.** Paste images and log files directly into prompts. Agents consume these as context for debugging.

### P4: Intelligence and observability

**Repo onboarding and scoring.** Automated analysis when Tekton first encounters a repo: language, framework, test coverage, CI setup, build system. Stored as metadata for better agent context and task complexity estimation.

**Audit log depth and retention.** Expand baseline audit logs with policy decision traces, retention controls, and export/search for compliance workflows.

**Agent quality metrics.** Merge rate, review cycles needed, time from task creation to merge, success rate by repo and task type. Real data on whether agents are helping or creating review burden.

**Adoption and impact KPIs.** Track sessions-to-merged-PR conversion, weekly active prompting users, and median time-to-first-reviewable-PR as top-level product health metrics.

**Org-wide intelligence.** An AI that watches all prompts, task outcomes, and repos across the org. Surfaces patterns: "your team keeps fixing flaky tests in repo X, the root cause is Y", "this repo generates 3x more failed tasks than average", "agents struggle with Z, add it to agent memory." Turns logs and metrics into actionable insights.

**Task forking and checkpoints.** Save checkpoints during execution. Fork from any point to try a different approach without starting over. Explore multiple solutions in parallel.

**Merge conflict resolution.** Detect overlapping file changes before they happen, serialize agents touching the same areas, and when conflicts do occur, spawn a resolution agent that rebases and fixes them.

### Deprioritized for now

- **UI-only quality-of-life items** that do not improve safety, planning quality, or workflow adoption for teams at scale.
- **Advanced autonomy features** without matching controls, auditability, and rollback paths.
- **Provider-specific lock-in features** that reduce portability across models and runtimes.

### Limitations (current)

- Single-node installs are the primary supported path.
- Cloud provider provisioning is not automated yet.
- Multi-model orchestration is planned, not shipped.

### Known bugs

- **Fix screenshot capture.** Screenshots currently fail inside the Nix VM because the browser runs as root without `--no-sandbox`. Fix the sandboxing setup.
- **VM startup logs.** Surface Nix VM boot and startup logs in the UI so users can debug startup failures without SSH access to the host.
- **Remove update button.** Simplify the UI by removing the update button to avoid accidental or confusing state changes.

### Backlog: Dashboard and UX

- **In-preview console.** Terminal inside the preview environment to interact with the running deployment without leaving Tekton.
- **Per-project environment variables.** Env vars per project with task-specific overrides, available to agents and the preview console.
- **Dynamic task names.** Task names update to reflect current agent activity for an at-a-glance view of active work.
- **One-click PR creation.** Button to create a PR from a completed task, pre-filled from the conversation history.
- **Branch selector dropdown.** Dropdown that filters branches as you type, defaults to `main`.
- **Custom task naming.** Set a custom name at creation time instead of auto-generating from the prompt.
- **Task-to-PR traceability.** Show a link back to the originating Tekton task alongside the PR link.
- **Mark task as failed.** Explicit failed state, separate from completed or abandoned.
- **Run real-world apps.** Full production-like applications in preview environments with databases, dependencies, and services.

## Documentation

- **[Deployment Guide](docs/deployment-guide.md)**: Full step-by-step setup for a new Hetzner server
- **[Preview Deployments](docs/preview-deployments.md)**: PR preview system, webhook setup, commands reference
- **[Architecture](docs/architecture.md)**: System design, networking, key decisions

## Directory Structure

```
tekton/
├── README.md
├── setup.sh                          # Automated setup script (--vertex for Elixir support)
├── docs/
│   ├── deployment-guide.md           # Full deployment walkthrough
│   ├── preview-deployments.md        # Preview system documentation
│   └── architecture.md               # System architecture overview
├── dashboard/
│   ├── backend/                      # Rust (Axum) API server
│   └── frontend/                     # React + shadcn/ui dashboard
├── initial-install/                  # Used once for nixos-anywhere installation
│   ├── flake.nix
│   ├── disk-config.nix               # RAID 1 across two SSDs
│   └── configuration.nix
└── server-config/                    # Copied to /etc/nixos after install
    ├── flake.nix                     # Nix flake (host + all container configs)
    ├── configuration.nix             # Host server config
    ├── agent-config.nix              # Agent container config
    ├── agent.sh                      # Agent lifecycle helper
    ├── preview-config.nix            # Node.js preview container config
    ├── vertex-preview-config.nix     # Vertex preview container config
    ├── preview.sh                    # Preview lifecycle helper
    └── preview-webhook/              # GitHub webhook service (Fastify/TypeScript)
        ├── src/
        │   ├── index.ts              # Webhook server + PR event handler
        │   ├── github.ts             # Signature verification, event parsing
        │   ├── preview.ts            # Shells out to `preview` CLI
        │   └── config.ts             # Environment variable loading
        └── package.json
```

## References

**Used in this project:**
- [nixos-anywhere](https://github.com/nix-community/nixos-anywhere): Remote NixOS installation over SSH
- [NixOS Containers](https://wiki.nixos.org/wiki/NixOS_Containers): Imperative container management with `nixos-container`
- [Michael Stapelberg: Running coding agents in NixOS MicroVMs](https://michael.stapelberg.ch/posts/2026-02-01-coding-agent-microvm-nix/): Original inspiration

**Related reading:**
- [Running NixOS from any Linux Distro in systemd-nspawn Containers](https://nixcademy.com/posts/nixos-nspawn/): Alternative approach using `machinectl` with pre-built images
- [nspawn-nixos](https://github.com/tfc/nspawn-nixos): Pre-built NixOS images for systemd-nspawn
- [Why We Built Our Own Background Agent](https://builders.ramp.com/post/why-we-built-our-background-agent): Ramp's experience building an internal background coding agent

**Competitive references (roadmap benchmarks):**
- [GitHub Copilot Coding Agent](https://docs.github.com/en/copilot/how-tos/use-copilot-agents/coding-agent): Agent workflow, governance, and enterprise controls
- [OpenHands Documentation](https://docs.openhands.dev/): Open-source autonomous coding agent patterns (skills, runtime, deployment)
- [Cursor Background Agents](https://docs.cursor.com/en/background-agents): Team workflows for delegated coding tasks
- [Devin Documentation](https://docs.devin.ai/): Ask/planning workflows and session-to-playbook style automation
- [Factory Documentation](https://docs.factory.ai/welcome): Model-agnostic coding agent workflows for teams
- [Codegen Overview](https://docs.codegen.com/introduction/overview): Agent platform primitives (integrations, permissions, sandboxes)
- [OpenAI Codex Updates](https://openai.com/index/introducing-upgrades-to-codex/): Permissioning and sandbox model direction

**Additional open-source competitors to track:**
- [OpenHands (GitHub)](https://github.com/All-Hands-AI/OpenHands): Open agent platform with CLI/GUI/cloud and enterprise self-hosting path
- [Continue (GitHub)](https://github.com/continuedev/continue): Open-source agent workflows across IDE, CLI, and headless/background execution
- [SWE-agent (GitHub)](https://github.com/SWE-agent/SWE-agent): Research-heavy autonomous issue-resolution agent with strong benchmarking focus
- [Aider (GitHub)](https://github.com/Aider-AI/aider): Terminal-native coding agent with strong git-centric workflow and broad model support
- [Goose (GitHub)](https://github.com/block/goose): Extensible local coding agent with MCP integrations and reusable recipe configs
- [Cline (GitHub)](https://github.com/cline/cline): IDE-native autonomous agent with explicit human approval model
- [Plandex (GitHub)](https://github.com/plandex-ai/plandex): Large-context terminal agent focused on planning, sandboxed diffs, and controllable autonomy
