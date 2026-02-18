# Architecture

Overview of the system architecture, networking, and key design decisions.

## Component Overview

```
┌─────────────────────────────────────────────────────────────────────┐
│  Hetzner Dedicated Server (NixOS)                                   │
│                                                                     │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────────────────┐  │
│  │ Caddy        │  │ Preview      │  │ PostgreSQL               │  │
│  │ (reverse     │  │ Webhook      │  │ (host, for node previews)│  │
│  │  proxy, TLS) │  │ (:3100)      │  │                          │  │
│  └──────┬───────┘  └──────┬───────┘  └──────────────────────────┘  │
│         │                 │                                         │
│         │  routes to containers by subdomain                        │
│         │                                                           │
│  ┌──────┴───────────────────────────────────────────────────────┐   │
│  │  10.100.0.0/24 veth network (NAT to external interface)      │   │
│  │                                                               │   │
│  │  ┌─────────────────┐  ┌─────────────────┐  ┌────────────┐   │   │
│  │  │ Agent Container  │  │ Node Preview    │  │ Vertex     │   │   │
│  │  │ 10.100.0.3      │  │ 10.100.0.5      │  │ Preview    │   │   │
│  │  │                  │  │                  │  │ 10.100.0.7 │   │   │
│  │  │ - Claude Code    │  │ - Node.js app   │  │            │   │   │
│  │  │ - Dev tools      │  │   (:3000)       │  │ - Phoenix  │   │   │
│  │  │ - SSH            │  │                  │  │   (:4000)  │   │   │
│  │  │                  │  │                  │  │ - React    │   │   │
│  │  │                  │  │                  │  │   (:3000,  │   │   │
│  │  │                  │  │                  │  │    :3001)  │   │   │
│  │  │                  │  │                  │  │ - PG+Redis │   │   │
│  │  └─────────────────┘  └─────────────────┘  └────────────┘   │   │
│  └───────────────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────────────┘
```

## Network Architecture

### Subnet: `10.100.0.0/24`

All containers (agents and previews) share a single private subnet. Each container gets a pair of IPs allocated from a shared counter (`/var/lib/claude-agents/.next_slot`):

| Slot | Host-side IP | Container IP | Purpose |
|------|-------------|--------------|---------|
| 1 | 10.100.0.2 | 10.100.0.3 | First container |
| 2 | 10.100.0.4 | 10.100.0.5 | Second container |
| N | 10.100.0.{N*2} | 10.100.0.{N*2+1} | Nth container |

The slot counter only increments (never reuses), so destroyed containers leave gaps. This is intentional — it avoids IP conflicts without needing a full allocation table.

### NAT

Containers reach the internet through NAT:

```nix
networking.nat = {
  enable = true;
  internalInterfaces = [ "ve-+" ];  # all container veth interfaces
  externalInterface = "enp3s0";     # server's physical interface
};
```

### Firewall

The host firewall allows:
- Ports 80 (HTTP, for ACME challenges) and 443 (HTTPS) from the internet
- All traffic from container veth interfaces (`ve-+`) is trusted — this is required for containers to reach host PostgreSQL and other services

```nix
networking.firewall.allowedTCPPorts = [ 80 443 ];
networking.firewall.trustedInterfaces = [ "ve-+" ];
```

### DNS and TLS

Caddy handles TLS automatically:
- Wildcard subdomain DNS (`*.preview.example.com`) points to the server IP
- Caddy gets certificates from Let's Encrypt
- Each preview gets a Caddy config file in `/etc/caddy/previews/<slug>.caddy`
- The main Caddy config imports all files: `import /etc/caddy/previews/*.caddy`

## Request Flow

### Preview Webhook Flow

```
GitHub PR event
  → HTTPS → Caddy (webhook.preview.example.com)
    → reverse_proxy → localhost:3100 (preview-webhook service)
      → verifies HMAC-SHA256 signature
      → returns 202 immediately
      → background: runs `preview create/update/destroy` via shell
        → nixos-container create/start/stop/destroy
        → writes/removes Caddy route files
        → reloads Caddy
```

### Preview Request Flow

```
Browser: https://42.preview.example.com
  → Caddy (TLS termination, reads /etc/caddy/previews/42.caddy)
    → reverse_proxy → 10.100.0.5:3000 (node preview)

Browser: https://42.preview.example.com/api/users
  → Caddy (vertex routing)
    → reverse_proxy → 10.100.0.7:4000 (Phoenix backend)
```

## Key Files

### Host Configuration

| File | Purpose |
|------|---------|
| `server-config/configuration.nix` | Host NixOS config (networking, Caddy, PostgreSQL, systemd services) |
| `server-config/flake.nix` | Nix flake defining all system configurations |
| `server-config/agent.sh` | Agent container lifecycle manager (create/destroy/list/ssh) |
| `server-config/preview.sh` | Preview container lifecycle manager (create/destroy/update/list/logs) |

### Container Configurations

| File | Flake output | Purpose |
|------|-------------|---------|
| `server-config/agent-config.nix` | `nixosConfigurations.agent` | Agent container (Claude Code + dev tools) |
| `server-config/preview-config.nix` | `nixosConfigurations.preview` | Node.js preview container |
| `server-config/vertex-preview-config.nix` | `nixosConfigurations.vertex-preview` | Vertex preview container (Elixir + React) |

### Webhook

| File | Purpose |
|------|---------|
| `server-config/preview-webhook/src/index.ts` | Fastify server, webhook endpoint |
| `server-config/preview-webhook/src/github.ts` | HMAC signature verification, PR event parsing |
| `server-config/preview-webhook/src/preview.ts` | Shells out to `preview` CLI, adds PR links |
| `server-config/preview-webhook/src/config.ts` | Environment variable loading |

### Initial Install

| File | Purpose |
|------|---------|
| `initial-install/flake.nix` | Flake for nixos-anywhere (references disko for disk setup) |
| `initial-install/disk-config.nix` | RAID 1 disk layout across two SSDs |
| `initial-install/configuration.nix` | Minimal NixOS config for first boot |

## Key Design Decisions

### Pre-built System Closures

Container creation uses pre-built Nix closures instead of evaluating the flake each time:

```bash
# Build once (or done automatically on first create):
nix build /etc/nixos#nixosConfigurations.agent.config.system.build.toplevel

# Create instantly using the cached store path:
nixos-container create myagent --system-path /nix/store/...
```

The store path is cached in `/var/lib/claude-agents/.system-path` (agents) and `/var/lib/preview-deploys/.system-path` (previews). This makes agent creation take ~3 seconds instead of minutes.

### `writeShellApplication` for CLI Tools

The `agent` and `preview` commands are wrapped with `writeShellApplication`, which:
- Adds runtime dependencies to `PATH` automatically
- Enforces shellcheck at build time (unquoted variables fail the build)
- Creates a proper executable in the system path

```nix
(pkgs.writeShellApplication {
  name = "preview";
  runtimeInputs = [ coreutils gnused nixos-container openssh curl jq postgresql sudo ];
  text = builtins.readFile ./preview.sh;
})
```

### Imperative Containers

The project uses NixOS imperative containers (`nixos-container create/destroy`) rather than declarative ones in `configuration.nix`. This allows:
- Creating/destroying containers without `nixos-rebuild switch`
- Dynamic container names and IPs
- Instant creation from pre-built closures

### Environment Injection via `/etc/preview.env`

Preview containers receive their configuration through `/etc/preview.env`, written into the container's filesystem before it starts. This avoids needing to pass environment variables through `nixos-container` or modify the container config per-instance.

### Vertex: Container-Local PostgreSQL

Vertex previews run their own PostgreSQL inside the container, unlike node previews which use the host's shared PostgreSQL. This was chosen because:
- Vertex uses `trust` authentication locally (simpler than managing per-container passwords)
- Full isolation — destroying the container cleans up everything
- No cross-container database access concerns

### IP Allocation: Monotonic Counter

IPs are allocated from a monotonic counter rather than scanning for free slots. This means:
- Destroyed containers leave IP gaps (e.g., slot 3 is never reused if container 3 is destroyed)
- Simple and race-free — no need to lock or scan
- The `/24` subnet supports ~127 concurrent containers, which is more than enough
