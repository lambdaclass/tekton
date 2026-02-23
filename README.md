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

### P0: Core platform (build next)

**Multi-model support.** Claude is the default agent, but the platform should support multiple providers. Each user connects their own API accounts (Claude, ChatGPT, Gemini, etc.) and Tekton auto-detects access based on their email. For overflow or users without keys, a shared pool routes through OpenRouter with model selection. Configuration is per-org: which models are available, which is the default, and spending limits per user/team.

**Elastic infrastructure.** Hetzner and OVH bare metal servers are the primary compute backend. You configure a base fleet (e.g., 3 bare metal machines) and Tekton manages them as a pool. When demand exceeds capacity, it provisions VPS instances (AWS, GCP, Hetzner Cloud, OVH Cloud) and runs NixOS VMs inside them. When demand drops, the elastic nodes are torn down. All declarative in a config file: base fleet size, max elastic nodes, provider credentials, and scaling thresholds.

**Task and prompt persistence.** Every prompt, response, agent action, and conversation thread is stored in a database with full history. Nothing is lost when a container is destroyed. Tasks are queryable and searchable across the org. This is the foundation that makes agent memory, duplicate detection, org-wide intelligence, and audit logging possible.

**Conversational threads on PRs.** Every task has a conversation thread, similar to GitHub PR discussions. Anyone on the team can jump into a running task, add follow-up prompts, see the full history of every prompt and response, and see every preview URL that was generated along the way. The thread is the source of truth for what was asked, what was done, and what the result was. Every message shows who submitted it so the history reads like a real conversation with clear accountability.

**Real-time collaboration.** Everything happens in real-time via WebSockets. When one person sends a follow-up prompt, everyone watching sees it. When the agent writes code, everyone sees the logs stream. Uses Operational Transformation for shared state. A central server determines ordering, which is fine since Tekton is the server. Multiple users can be active in the same task simultaneously.

**Draft/plan mode.** Before an agent starts coding, it can run in draft mode: it reads the codebase, proposes a plan (files to change, approach, tradeoffs), and publishes it as a draft with rendered Markdown. Team members can comment, suggest changes, approve or reject. Comments are fed back to the agent, which can regenerate the plan. Only after approval does the agent start writing code. This prevents wasted compute, catches bad approaches early, and gives the team visibility into what's about to happen. Configurable per task: skip straight to coding for simple fixes, require plan approval for larger changes.

**Security and access control.** RBAC for teams and orgs: who can create tasks, which repos they can target, who can approve plans, who can see costs. Secret management for agents that need credentials during tasks (database passwords, internal API keys, service tokens). Secrets are injected into the container at runtime, never stored in prompts or logs.

**Private workspaces.** Some work shouldn't be visible to the whole org. A team or group of people can create a private workspace tied to a repo or conversation, like a private Google Doc. Only invited members see the tasks, prompts, agent output, and previews. Everyone else sees nothing. Use case: management working on financial modeling, a security team investigating a vulnerability, or a small group prototyping something before sharing it. Visibility is per-workspace, not just per-repo.

### P1: Multiplier features (high leverage)

**CI integration and agent self-correction.** After an agent opens a PR, watch CI results. If tests fail, the agent picks up the failure logs and iterates automatically. If a human leaves review comments, the agent addresses them without needing a new prompt. The loop ends at "PR merged", not "PR created."

**GitHub App integration.** Beyond webhooks, Tekton becomes a GitHub App. Comment `/tekton fix this` on an issue and it picks up the task. Comment `/tekton address this feedback` on a PR review and it iterates. The conversational threads live both in Tekton and in GitHub with comments syncing bidirectionally. This turns Tekton from a tool you go to into a tool that meets you where you already work.

**Agent memory.** Each task currently starts from scratch. Instead, maintain a per-repo knowledge base that persists across tasks: architecture notes, conventions, past mistakes, debugging insights. When an agent starts a new task on a repo it's seen before, it gets this context injected automatically. The knowledge base grows with every task.

**Agent tool ecosystem.** Right now agents can only write code. But sometimes they need to run a database migration, query a monitoring dashboard, check Sentry for related errors, or call an internal API. An MCP-style plugin system where you register tools agents can use per-repo or per-org. This is what separates a coding agent from an engineering agent.

**Queue management and priority.** A proper job queue with priority levels (urgent, normal, background), fair scheduling across users, and preemption. High-priority tasks can bump low-priority ones when capacity is constrained. Visibility into queue depth and wait times.

**Cost tracking and budgets.** With multiple API providers and elastic infrastructure, cost visibility is essential. Track token usage per task, compute time per container, and aggregate by user and team. Org admins set spending limits and get alerts. The dashboard shows burn rate, cost per merged PR, and trends over time.

**Duplicate work detection.** An AI watcher monitors all active and recent prompts across the org in real-time. When someone submits a task that overlaps with something already in progress or recently completed, Tekton flags it: "Alice is already working on something similar in task X" or "This was addressed 2 days ago in PR #123." Prevents two people from unknowingly asking agents to do the same thing, saves compute, and surfaces opportunities to collaborate. Uses embeddings over prompt + repo + file paths for semantic similarity, not just keyword matching.

**Local model support.** Beyond cloud APIs, support self-hosted models (Llama, DeepSeek, Qwen, etc.) running on the same infrastructure or a dedicated GPU node. Important for teams that can't send code to external providers, and for cutting costs on simpler tasks that don't need frontier models.

### P2: Workflow and integrations

**Approval gates.** Before an agent pushes or creates a PR, optionally pause and present the diff for human review. Configurable per repo or per team. Some want full autonomy, others want a checkpoint. The approval can happen in the dashboard, in Slack, or via a GitHub review.

**Rollback/undo.** One-click revert of everything an agent did: close the PR, delete the branch, destroy the preview, undo the commits. Clean, total undo. No manual cleanup.

**Notifications and integrations.** Slack, Discord, and email notifications when tasks complete, fail, or need follow-up. Outgoing webhook events (task.completed, preview.ready, task.failed) that other systems can subscribe to for custom workflows.

**Task templates and playbooks.** Recurring patterns become reusable templates: "upgrade dependency X across all repos", "add an endpoint with tests", "fix this Sentry error". Users save prompt templates with variable slots. Repos can ship a `.tekton/playbooks/` directory with predefined tasks that show up in the UI, making common operations one-click.

**Scheduled and recurring tasks.** Cron-like jobs: "every Monday, check for dependency updates", "after every release, update the changelog", "scan for deprecated API usage weekly." Define them in the dashboard or in a `.tekton/schedules` config in the repo.

**Event-driven tasks.** Beyond scheduled jobs, trigger tasks from external events. A Sentry error creates a task to fix it. A PagerDuty alert spawns a debugging agent. A Slack message kicks off a task. Tekton becomes reactive to what's happening in production, not just what humans remember to ask for.

**Code review as a task type.** Not just writing code, but reviewing it. "Review this PR for security issues." "Check if this PR follows our conventions." Turns agents into reviewers, not just authors. Useful for teams that want a first-pass review before a human looks at it.

**Multi-repo tasks.** Some changes span repositories (update an API and its client library, change a shared schema and all consumers). A single task should be able to coordinate agents across multiple repos, with each agent aware of what the others are doing, and the results linked together.

**Monorepo support.** Large monorepos need special handling: partial clones, understanding package boundaries, running only affected tests, knowing which team owns which directory. Without this, agents waste time and tokens on irrelevant code.

**Image and log attachments in prompts.** Paste images and log files directly into prompts. Agents consume these as context: paste a screenshot of a bug or error logs and the agent uses them to debug.

### P3: Intelligence and observability

**Repo onboarding and scoring.** When Tekton first encounters a repo, run an automated analysis: language, framework, test coverage, CI setup, build system. Store this as metadata to give future agents better context and to estimate task complexity before it starts.

**Audit log.** Every action, every prompt, every API call. Who did what, when, and what happened. Essential for teams with compliance requirements, but also useful for debugging and understanding how the platform is being used.

**Agent quality metrics.** Track how often agent PRs get merged vs rejected, average review cycles needed, time from task creation to merge, and which repos or task types agents handle well vs poorly. Gives you real data on whether agents are helping or just creating review burden.

**Org-wide intelligence.** An AI that watches everything happening across the org: all prompts, all task outcomes, all repos. Surfaces patterns humans would miss. "Your team keeps asking agents to fix flaky tests in repo X, the root cause is Y." "This repo generates 3x more failed tasks than average, here's what's different." "Based on the last 50 tasks, agents struggle with Z, consider adding it to the repo's agent memory." Turns the audit log and metrics into actionable insights instead of just data.

**Task forking and checkpoints.** "I liked what the agent did until step 3, fork from there and try a different approach." Save checkpoints during execution so you can branch task history and explore multiple solutions without starting over.

**Merge conflict resolution.** When multiple agents work on the same repo simultaneously, their branches will conflict. Tekton should detect overlapping file changes before they happen, serialize agents touching the same areas, and when conflicts do occur, spawn a resolution agent that rebases and fixes them automatically.

### Backlog: Dashboard and UX improvements

- **In-preview console.** A terminal inside the preview environment so users can interact with the running deployment directly without leaving Tekton.
- **Per-project environment variables.** Configure env vars per project with task-specific overrides, available both to the agent while coding and in the preview console.
- **Dynamic task names.** Task names update to reflect what the agent is currently working on, so the task list gives an at-a-glance view of active work.
- **One-click PR creation.** A button to create a GitHub PR directly from a completed task, pre-filled from the task context and conversation history.
- **Branch selector dropdown.** Replace the plain text branch input with a dropdown that filters as you type and defaults to `main`.
- **Custom task naming.** Let users set a custom name for a task at creation time instead of auto-generating one from the prompt.
- **Task-to-PR traceability.** After the agent pushes a branch, show a link back to the Tekton task that originated the work alongside the PR link.
- **Mark task as failed.** Add an explicit failed state for tasks, separate from completed or abandoned, for tracking success rates and debugging.
- **Run real-world apps.** Support running full production-like applications (e.g., Escolaria) in preview environments with databases, dependencies, and services beyond simple dev servers.
- **Fix screenshot capture.** Screenshots currently fail inside the Nix VM because the browser runs as root without `--no-sandbox`. Fix the sandboxing setup.
- **VM startup logs.** Surface Nix VM boot and startup logs in the UI so users can debug startup failures without SSH access to the host.
- **Remove update button.** Simplify the UI by removing the update button to avoid accidental or confusing state changes.

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
