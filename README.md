# NixOS Setup for Claude Code Agents on Hetzner

> Inspired by [Michael Stapelberg's blog post](https://michael.stapelberg.ch/posts/2026-02-01-coding-agent-microvm-nix/) on running coding agents in NixOS. Uses [nixos-anywhere](https://github.com/nix-community/nixos-anywhere) for remote NixOS installation and imperative [systemd-nspawn](https://www.freedesktop.org/software/systemd/man/latest/systemd-nspawn.html) containers for lightweight, instant agent creation.

This repo sets up a Hetzner bare metal server with NixOS and nspawn containers for running Claude Code agents in isolated, ephemeral environments. It also supports PR preview deployments — both Node.js apps and Elixir/Phoenix (Vertex) monorepos — with automatic GitHub webhook integration.

## Documentation

- **[Deployment Guide](docs/deployment-guide.md)** — Full step-by-step setup for a new Hetzner server
- **[Preview Deployments](docs/preview-deployments.md)** — PR preview system, webhook setup, commands reference
- **[Architecture](docs/architecture.md)** — System design, networking, key decisions

## Directory Structure

```
nixos-claude/
├── README.md
├── setup.sh                          # Automated setup script (--vertex for Elixir support)
├── docs/
│   ├── deployment-guide.md           # Full deployment walkthrough
│   ├── preview-deployments.md        # Preview system documentation
│   └── architecture.md               # System architecture overview
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
