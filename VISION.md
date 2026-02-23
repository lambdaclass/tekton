# Tekton Vision

Tekton is a self-hosted platform for running background AI coding agents at scale. The goal is to make it the default way engineering teams interact with AI for code — not a chatbot you talk to, but infrastructure that builds things for you.

## Roadmap

### P0 — Core platform (build next)

**Multi-Model Support** — Claude is the default agent, but the platform should support multiple AI providers. Each user connects their own API accounts (Claude, ChatGPT, Gemini, etc.), and Tekton auto-detects access based on their email. For overflow or users without their own keys, a shared pool routes through OpenRouter with model selection. Configuration is per-org: which models are available, which is the default, and spending limits per user/team.

**Elastic Infrastructure** — Hetzner and OVH bare metal servers are the primary compute backend. You configure a base fleet (e.g., 3 bare metal machines) and Tekton manages them as a pool. When demand exceeds capacity, it elastically provisions VPS instances (AWS, GCP, Hetzner Cloud, OVH Cloud) and runs NixOS VMs inside them. When demand drops, the elastic nodes are torn down. All of this is declarative in a config file: base fleet size, max elastic nodes, provider credentials, and scaling thresholds.

**Conversational Threads on PRs** — Every task has a conversation thread, similar to GitHub PR discussions. Anyone on the team can jump into a running task, add follow-up prompts, see the full history of every prompt and response, and see every preview URL that was generated along the way. The thread is the source of truth for what was asked, what was done, and what the result was.

**Real-Time Collaboration** — Everything happens in real-time via WebSockets. When one person sends a follow-up prompt, everyone watching sees it. When the agent writes code, everyone sees the logs stream. Use Operational Transformation for shared state — a central server determines ordering, which is fine since Tekton is the server. Multiple users can be active in the same task simultaneously.

**Draft / Plan Mode** — Before an agent starts coding, it can run in draft mode: it reads the codebase, proposes a plan (files to change, approach, tradeoffs), and publishes it as a draft. Team members can comment, suggest changes, or approve — just like a PR review, but for the plan itself. Only after approval does the agent start writing code. This prevents wasted compute, catches bad approaches early, and gives the team visibility into what's about to happen. Configurable per task: skip straight to coding for simple fixes, require plan approval for larger changes.

**Duplicate Work Detection** — An AI watcher monitors all active and recent prompts across the org in real-time. When someone submits a task that overlaps with something already in progress or recently completed, Tekton flags it: "Alice is already working on something similar in task X" or "This was addressed 2 days ago in PR #123." This prevents two people from unknowingly asking agents to do the same thing, saves compute, and surfaces opportunities to collaborate. The watcher uses embeddings over prompt + repo + file paths to detect semantic similarity, not just keyword matching.

### P1 — Multiplier features (high leverage)

**GitHub App Integration** — Beyond webhooks, Tekton becomes a GitHub App. Comment `/tekton fix this` on an issue and it picks up the task. Comment `/tekton address this feedback` on a PR review and it iterates. The conversational threads live both in Tekton and in GitHub — comments sync bidirectionally. This is what turns Tekton from a tool you go to into a tool that meets you where you already work.

**Agent Memory** — Each task currently starts from scratch. Instead, maintain a per-repo knowledge base that persists across tasks: architecture notes, conventions, past mistakes, debugging insights. When an agent starts a new task on a repo it's seen before, it gets this context injected automatically. The knowledge base grows with every task.

**Queue Management & Priority** — A proper job queue with priority levels (urgent, normal, background), fair scheduling across users, and preemption — high-priority tasks can bump low-priority ones when capacity is constrained. Visibility into queue depth and wait times.

**Cost Tracking & Budgets** — With multiple API providers and elastic infrastructure, cost visibility is essential. Track token usage per task, compute time per container, and aggregate by user and team. Org admins set spending limits and get alerts. The dashboard shows burn rate, cost per merged PR, and trends over time.

### P2 — Polish & workflow (quality of life)

**Approval Gates** — Before an agent pushes or creates a PR, optionally pause and present the diff for human review. Configurable per repo or per team — some want full autonomy, others want a checkpoint. The approval can happen in the dashboard, in Slack, or via a GitHub review.

**Rollback / Undo** — One-click revert of everything an agent did: close the PR, delete the branch, destroy the preview, undo the commits. Clean, total undo. No manual cleanup.

**Notifications & Integrations** — Slack, Discord, and email notifications when tasks complete, fail, or need follow-up. Outgoing webhook events (task.completed, preview.ready, task.failed) that other systems can subscribe to for custom workflows.

**Task Templates & Playbooks** — Recurring patterns become reusable templates: "upgrade dependency X across all repos", "add an endpoint with tests", "fix this Sentry error". Users save prompt templates with variable slots. Repos can ship a `.tekton/playbooks/` directory with predefined tasks that show up in the UI, making common operations one-click.

### P3 — Intelligence & compliance

**Repo Onboarding & Scoring** — When Tekton first encounters a repo, run an automated analysis: language, framework, test coverage, CI setup, build system. Store this as metadata to give future agents better context and to estimate task complexity before it starts.

**Audit Log** — Every action, every prompt, every API call — who did what, when, and what happened. Essential for teams with compliance requirements, but also useful for debugging and understanding how the platform is being used.
