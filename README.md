# Tekton

> *tekton* (τέκτων) — Greek for "builder"

Tekton is a self-hosted platform for running background AI coding agents at scale. The goal is infrastructure that builds things for you — not a chatbot you talk to, but a system where you describe what you want, and agents write the code, open the PR, and deploy a preview.

Today it runs on NixOS with isolated systemd-nspawn containers on bare metal. An agent spins up in ~3 seconds, does the work unattended, and creates a PR with a live preview. A web dashboard lets you create tasks, watch logs in real time, and send follow-ups.

Inspired by [Michael Stapelberg's post on running coding agents in NixOS](https://michael.stapelberg.ch/posts/2026-02-01-coding-agent-microvm-nix/).

## What it does today

- **Background coding agents** — Each agent runs in its own isolated NixOS container, works unattended, and pushes results as a PR
- **PR preview deployments** — Automatic preview environments for Node.js and Elixir/Phoenix apps via GitHub webhooks
- **Web dashboard** — Create tasks, monitor live logs via WebSocket, send follow-up prompts, view preview screenshots
- **Voice input & repo auto-detection** — Speak your task, repo is classified automatically
- **Subtask spawning** — Agents can split work into parallel child tasks

## Roadmap

**P0 — Core platform**
- Multi-model support (Claude, ChatGPT, Gemini) with per-user API accounts and OpenRouter fallback
- Elastic infrastructure — bare metal base fleet with auto-scaling to cloud VPS on demand
- Conversational threads on PRs — full prompt/response history per task
- Real-time collaboration via WebSockets and Operational Transformation
- Draft/plan mode — agents propose a plan for review before writing code
- Duplicate work detection across the org using semantic similarity

**P1 — Multiplier features**
- GitHub App — trigger tasks from issue/PR comments (`/tekton fix this`)
- Agent memory — per-repo knowledge base that persists across tasks
- Queue management with priority levels and fair scheduling
- Cost tracking and per-user/team budgets

**P2 — Polish & workflow**
- Approval gates — human review before push/PR, configurable per repo
- One-click rollback of everything an agent did
- Slack, Discord, email notifications and outgoing webhooks
- Task templates and reusable playbooks

**P3 — Intelligence & compliance**
- Repo onboarding and complexity scoring
- Full audit log of every action, prompt, and API call

See **[Vision & Roadmap](VISION.md)** for full details on each item.

## Documentation

- **[Vision & Roadmap](VISION.md)** — Project direction, priorities, and detailed feature descriptions
- **[Deployment Guide](docs/deployment-guide.md)** — Full step-by-step setup for a new Hetzner server
- **[Preview Deployments](docs/preview-deployments.md)** — PR preview system, webhook setup, commands reference
- **[Architecture](docs/architecture.md)** — System design, networking, key decisions

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

## Prerequisites

- Local machine with [Nix installed](https://nixos.org/download/) (flakes enabled)
- SSH key pair (`ssh-keygen` if you don't have one)
- Hetzner dedicated server in rescue mode

## Quick Start

### 1. Provision and Setup

```bash
# Basic setup (agents only)
./setup.sh

# With Vertex preview support
./setup.sh --vertex
```

The script handles everything: network detection, NixOS installation, server configuration, and Claude login. See the [Deployment Guide](docs/deployment-guide.md) for details.

### 2. Use Agent Containers

```bash
# Create an agent (~3 seconds)
ssh root@YOUR_SERVER_IP 'agent create myagent'

# SSH into the agent
ssh -J root@YOUR_SERVER_IP agent@<container-ip>

# Run Claude
claude
claude --dangerously-skip-permissions  # headless mode

# List and destroy
ssh root@YOUR_SERVER_IP 'agent list'
ssh root@YOUR_SERVER_IP 'agent destroy myagent'
```

### 3. Preview Deployments

Previews are created automatically via GitHub webhook, or manually:

```bash
# Deploy a branch
ssh root@YOUR_SERVER_IP 'preview create owner/repo branch-name'

# Vertex (Elixir/Phoenix) preview
ssh root@YOUR_SERVER_IP 'preview create owner/repo branch --type vertex --slug pr-42'

# Manage previews
ssh root@YOUR_SERVER_IP 'preview list'
ssh root@YOUR_SERVER_IP 'preview logs pr-42 --follow'
ssh root@YOUR_SERVER_IP 'preview destroy pr-42'
```

See [Preview Deployments](docs/preview-deployments.md) for webhook setup and full reference.

## Troubleshooting

| Problem | Solution |
|---------|----------|
| Container fails to start | `journalctl -u container@<name>` on host |
| SSH asks for password | SSH key not baked in — run `agent build` after config changes, recreate |
| SSH host key warning | `ssh-keygen -R <container-ip>` |
| Claude "Invalid API key" | Re-run `claude login` on host, fix permissions, recreate container |
| Preview returns 502 | App is still building — `preview logs <slug> --follow` |
| Webhook not triggering | `systemctl status preview-webhook` and check GitHub webhook deliveries |

## Quick Reference

```bash
# Agent management (run on host as root)
agent create myagent
agent ssh myagent
agent list
agent destroy myagent
agent build                    # rebuild agent closure after config changes

# Preview management
preview create org/repo branch
preview list
preview logs <slug> --follow
preview update <slug>
preview destroy <slug>
preview build                  # rebuild preview closure
preview build --type vertex    # rebuild vertex closure

# Host maintenance
cd /etc/nixos && nixos-rebuild switch

# Re-authenticate Claude
CLAUDE_CONFIG_DIR=/var/secrets/claude claude login
chmod -R a+rX /var/secrets/claude
agent destroy myagent && agent create myagent  # recreate to pick up new creds
```

## References

**Used in this project:**
- [nixos-anywhere](https://github.com/nix-community/nixos-anywhere) — Remote NixOS installation over SSH
- [NixOS Containers](https://wiki.nixos.org/wiki/NixOS_Containers) — Imperative container management with `nixos-container`
- [Michael Stapelberg: Running coding agents in NixOS MicroVMs](https://michael.stapelberg.ch/posts/2026-02-01-coding-agent-microvm-nix/) — Original inspiration

**Related reading:**
- [Running NixOS from any Linux Distro in systemd-nspawn Containers](https://nixcademy.com/posts/nixos-nspawn/) — Alternative approach using `machinectl` with pre-built images
- [nspawn-nixos](https://github.com/tfc/nspawn-nixos) — Pre-built NixOS images for systemd-nspawn
