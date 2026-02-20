# Dashboard Deployment Guide

Complete guide to deploying the Preview Dashboard from scratch on a new server.

The dashboard is a web application for managing NixOS preview containers and running Claude coding tasks. It consists of a Rust (Axum) backend, a React/TypeScript frontend, and depends on several external services.

## Architecture Overview

```
                         Cloudflare (DNS + proxy)
                                  |
                           Caddy (TLS termination)
                          /                \
            dashboard.hipermegared.link    *.hipermegared.link
                     |                          |
              Dashboard (port 3200)      Preview containers
              - Rust/Axum backend        - Node.js or Vertex (Elixir)
              - React SPA                - One per PR/branch
              - SQLite DB
                     |
              Agent containers
              - Claude Code
              - Git operations
              - SSH signing
```

## Prerequisites

- A server running NixOS (tested on 24.11) with root SSH access
- A domain managed by Cloudflare (for DNS + TLS)
- A GitHub App installed on the target org (for repo access tokens)
- A Google Cloud project (for OAuth login)
- A Claude Code account with valid credentials

## External Services Setup

### 1. Cloudflare DNS

You need three DNS records for your domain (e.g., `hipermegared.link`):

| Type | Name | Content | Proxy |
|------|------|---------|-------|
| A | `dashboard` | `<server-ip>` | Proxied |
| A | `*` | `<server-ip>` | Proxied |
| A | `@` | `<server-ip>` | Proxied |

The wildcard record is needed for preview subdomains like `t-abc123.hipermegared.link`.

**Cloudflare Origin CA Certificate:**

1. Go to SSL/TLS > Origin Server > Create Certificate
2. Generate a certificate covering `*.hipermegared.link` and `hipermegared.link`
3. Save the certificate as `cloudflare-origin.pem` and the private key as `cloudflare-origin-key.pem`
4. These will be placed on the server at `/var/secrets/`

**SSL/TLS Settings:**
- SSL mode: **Full (strict)**
- Edge Certificates > Always Use HTTPS: **On**

### 2. GitHub App

Create a GitHub App in your org (Settings > Developer settings > GitHub Apps > New GitHub App):

- **Name:** Something like "Preview Bot"
- **Homepage URL:** `https://dashboard.yourdomain.com`
- **Webhook URL:** `https://dashboard.yourdomain.com/api/webhooks/github` (or your webhook endpoint)
- **Webhook secret:** Generate a random string (you'll need this later)
- **Permissions:**
  - Repository: Contents (Read & Write), Pull requests (Read & Write), Metadata (Read)
- **Subscribe to events:** Pull request
- **Where can this app be installed?** Only on this account

After creating:
1. Note the **App ID**
2. Generate and download a **Private Key** (.pem file)
3. Install the app on the target repositories
4. Note the **Installation ID** (visible in the URL when you click on the installation: `https://github.com/settings/installations/<ID>`)

### 3. Google OAuth

1. Go to [Google Cloud Console](https://console.cloud.google.com/)
2. Create a new project (or use an existing one)
3. Go to APIs & Services > Credentials > Create Credentials > OAuth 2.0 Client IDs
4. Application type: **Web application**
5. Authorized redirect URIs: `https://dashboard.yourdomain.com/api/auth/callback`
6. Note the **Client ID** and **Client Secret**

If you want to restrict login to a specific domain (e.g., `@lambdaclass.com`), the dashboard enforces this via the `ALLOWED_DOMAIN` environment variable.

### 4. Claude Code Credentials

You need valid Claude Code credentials on the server. The easiest way:

1. SSH into the server
2. Run `claude` and complete the login flow
3. The credentials will be saved to `~/.claude/`
4. Copy them to `/var/secrets/claude/`:
   ```bash
   mkdir -p /var/secrets/claude
   cp ~/.claude/.claude.json /var/secrets/claude/
   cp ~/.claude/.credentials.json /var/secrets/claude/
   cp -r ~/.claude/cache /var/secrets/claude/ 2>/dev/null || true
   ```

### 5. SSH Commit Signing Key

Agent containers sign git commits with an SSH key so they can push to repos with signed-commit requirements.

1. Generate the key on the server:
   ```bash
   mkdir -p /var/secrets/claude-signing
   ssh-keygen -t ed25519 -f /var/secrets/claude-signing/signing_key -N "" -C "claude-dashboard-signing"
   cp /var/secrets/claude-signing/signing_key /var/secrets/claude/signing_key
   ```

2. Add the **public key** to GitHub as a **Signing Key**:
   - Go to https://github.com/settings/keys
   - Click "New SSH key"
   - Key type: **Signing Key**
   - Paste the contents of `/var/secrets/claude-signing/signing_key.pub`

## Server Setup

### Directory Structure

```
/opt/dashboard/
  dashboard          # Rust binary
  static/            # Frontend build (index.html, assets/)

/opt/preview-webhook/
  dist/              # Compiled TypeScript
  node_modules/
  package.json

/var/secrets/
  dashboard.env      # Dashboard environment variables
  preview.env        # Preview webhook environment variables
  cloudflare-origin.pem      # Cloudflare Origin CA cert
  cloudflare-origin-key.pem  # Cloudflare Origin CA key
  claude/            # Claude Code credentials
    .claude.json
    .credentials.json
    signing_key      # SSH signing key for git commits
    cache/
  github-app.pem     # GitHub App private key (if using App auth)

/var/lib/dashboard/
  dashboard.db       # SQLite database (created automatically)

/etc/nixos/
  configuration.nix  # Main NixOS config
  agent-config.nix   # Agent container NixOS config
  agent.sh           # Agent lifecycle script
  preview-config.nix # Node.js preview container config
  vertex-preview-config.nix  # Vertex preview container config
  preview.sh         # Preview lifecycle script
  flake.nix          # Nix flake for building container closures
```

### Environment Files

**`/var/secrets/dashboard.env`:**

```bash
LISTEN_ADDR=0.0.0.0:3200
DATABASE_URL=sqlite:///var/lib/dashboard/dashboard.db
JWT_SECRET=<random-64-char-hex-string>
GOOGLE_CLIENT_ID=<from-google-cloud-console>
GOOGLE_CLIENT_SECRET=<from-google-cloud-console>
GOOGLE_REDIRECT_URI=https://dashboard.yourdomain.com/api/auth/callback
ALLOWED_DOMAIN=yourdomain.com
PREVIEW_DOMAIN=yourdomain.com
ALLOWED_REPOS=yourorg/repo1,yourorg/repo2
VERTEX_REPOS=yourorg/elixir-repo
PREVIEW_BIN=/run/current-system/sw/bin/preview
AGENT_BIN=/run/current-system/sw/bin/agent
STATIC_DIR=/opt/dashboard/static
```

**`/var/secrets/preview.env`:**

```bash
# Option A: GitHub App (recommended)
GITHUB_APP_ID=<app-id>
GITHUB_APP_INSTALLATION_ID=<installation-id>
GITHUB_APP_PRIVATE_KEY_PATH=/var/secrets/github-app.pem

# Option B: Personal Access Token
# GITHUB_TOKEN=ghp_xxxxxxxxxxxx

GITHUB_WEBHOOK_SECRET=<webhook-secret-from-github-app>
PREVIEW_DOMAIN=yourdomain.com
WEBHOOK_PORT=3100
ALLOWED_REPOS=yourorg/repo1,yourorg/repo2
VERTEX_REPOS=yourorg/elixir-repo
```

### NixOS Configuration

The `configuration.nix` file includes:
- **Dashboard systemd service** — runs `/opt/dashboard/dashboard` with `EnvironmentFile=/var/secrets/dashboard.env`
- **Preview webhook systemd service** — runs the Node.js webhook
- **Caddy reverse proxy** — routes `dashboard.yourdomain.com` to port 3200, imports per-preview Caddy configs
- **NAT** — enables container internet access via `ve-+` interfaces
- **PostgreSQL** — shared database for preview containers
- **`agent` and `preview` CLI tools** — wrapped shell scripts for container management

**Important:** `configuration.nix` in this repo has placeholder values (`YOUR.SERVER.IP.HERE`, `ssh-ed25519 AAAA... your-key-here`, etc.). The setup script substitutes these with real values. **Never scp the template directly to the server.**

### Building and Deploying

#### First-time setup

Use the `setup.sh` script (see below) which handles:
1. Substituting placeholder values in configuration files
2. Creating secrets directories and environment files
3. Uploading all NixOS configs
4. Running `nixos-rebuild switch`
5. Building agent and preview container closures
6. Building and deploying the dashboard binary and frontend

#### Subsequent deploys (dashboard code changes only)

The backend cannot be cross-compiled from macOS (no musl toolchain), so it's built on the server:

```bash
# Build frontend locally
cd dashboard/frontend && npm run build

# Upload source to server
scp -r dashboard/backend/src root@<server>:/opt/dashboard-build/
scp dashboard/backend/Cargo.toml dashboard/backend/Cargo.lock root@<server>:/opt/dashboard-build/

# Upload frontend
scp -r dashboard/frontend/dist/index.html dashboard/frontend/dist/vite.svg root@<server>:/opt/dashboard/static/
scp -r dashboard/frontend/dist/assets root@<server>:/opt/dashboard/static/

# Build on server
ssh root@<server> 'cd /opt/dashboard-build && nix-shell -p rustc cargo gcc pkg-config --run "cargo build --release"'

# Deploy
ssh root@<server> 'systemctl stop dashboard && cp /opt/dashboard-build/target/release/dashboard /opt/dashboard/dashboard && systemctl start dashboard'
```

#### NixOS config changes

```bash
# For agent-config.nix, preview-config.nix, etc.:
scp server-config/<file> root@<server>:/etc/nixos/<file>
ssh root@<server> 'cd /etc/nixos && nixos-rebuild switch'

# If agent-config.nix changed, also rebuild the agent closure:
ssh root@<server> 'agent build'

# If vertex-preview-config.nix changed, also rebuild:
ssh root@<server> 'preview build --type vertex'
```

#### Preview webhook changes

```bash
scp -r server-config/preview-webhook/src server-config/preview-webhook/package*.json root@<server>:/opt/preview-webhook/
ssh root@<server> 'cd /opt/preview-webhook && npm ci && npm run build && systemctl restart preview-webhook'
```

## How Tasks Work

When a user submits a task via the dashboard:

1. **Create agent container** — `agent create t-<6chars>`, an isolated NixOS container with Claude Code, git, and dev tools
2. **Clone repo** — SSH into agent, clone the target repo, create branch `claude/<6chars>`
3. **Run Claude** — `claude --dangerously-skip-permissions -p '<prompt>'` inside the agent
4. **Commit changes** — `git add -A && git commit` (commits are SSH-signed)
5. **Push branch** — push to GitHub using a fresh token from the webhook's `/internal/token` endpoint
6. **Destroy agent** — clean up the agent container
7. **Create preview** — `preview create <repo> claude/<6chars> --slug t-<6chars> --type <node|vertex>`

The task status and all log output are stored in SQLite and streamed to the browser via WebSocket.

## Troubleshooting

### Dashboard won't start
```bash
systemctl status dashboard
journalctl -u dashboard -n 50
```
Check that `/var/secrets/dashboard.env` exists and has all required variables.

### Agent containers can't reach the internet
Check NAT is enabled:
```bash
sysctl net.ipv4.ip_forward   # Should be 1
iptables -t nat -L POSTROUTING  # Should show MASQUERADE rule
```

### Claude doesn't make changes in tasks
- Check credentials: `ls -la /var/secrets/claude/`
- Check token expiry in `.credentials.json` (`expiresAt` field, milliseconds since epoch)
- If expired, re-login: `claude` on the host, then copy credentials again

### Push rejected (signed commits required)
- Verify the signing key is in `/var/secrets/claude/signing_key`
- Verify the public key is added to GitHub as a **Signing Key** (not Authentication Key)
- The commit email in `agent-config.nix` must match the GitHub account that has the signing key

### Preview shows 502 Bad Gateway
The app inside the container hasn't started yet (especially vertex previews which take several minutes to build):
```bash
nixos-container run <slug> -- systemctl status setup-vertex   # or setup-preview
nixos-container run <slug> -- journalctl -u setup-vertex -n 30
```
