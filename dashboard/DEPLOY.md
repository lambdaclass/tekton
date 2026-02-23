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
              Agent containers (a-XXXXXX)
              - Claude Code (auth via CLAUDE_CODE_OAUTH_TOKEN)
              - Git operations (SSH-signed commits)
              - Created per task, destroyed after completion

              Preview containers (t-XXXXXX)
              - Created after first Claude run
              - Updated after each follow-up
              - Persist after task completion
```

### Task Pipeline

When a user submits a task, the pipeline runs:

```
1. Create agent container (a-{short_id})
2. Clone repo, create branch claude/{short_id}
3. Run Claude (stream-json) → commit → push → create preview (t-{short_id}) → screenshot
4. Follow-up loop:
   a. Status → awaiting_followup
   b. Wait for message (poll 10s, 5min timeout)
   c. If message: run Claude → commit → push → update preview → screenshot → go to 4a
   d. If done/timeout: break
5. Destroy agent container → status: completed
```

Key points:
- **Agent and preview have different prefixes** — `a-` for agents, `t-` for previews, so they don't collide
- **Push + preview happen after EACH Claude run**, not just at the end — users see results before deciding on follow-ups
- **Claude's actions are streamed** via `--output-format stream-json --verbose`, parsed into human-readable log lines (e.g., `⚡ Reading src/foo.tsx`, `✏️ Editing src/bar.tsx`)
- **Claude authenticates via `CLAUDE_CODE_OAUTH_TOKEN` env var**, passed through the SSH command by the dashboard backend (reads from `/var/secrets/claude/oauth_token` on the host)

## Prerequisites

- A server running NixOS (tested on 24.11) with root SSH access
- A domain managed by Cloudflare (for DNS + TLS)
- A GitHub App installed on the target org (for repo access tokens)
- A Google Cloud project (for OAuth login)
- A Claude Max/Teams subscription (for `claude setup-token`)

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

### 4. Claude Code Authentication

Claude Code in agent containers authenticates via a **long-lived OAuth token** (valid for 1 year) tied to your Claude Max/Teams subscription. This is NOT an API key — it uses your subscription, not per-token billing.

**Generate the token on your local machine:**

```bash
claude setup-token
```

This outputs a token starting with `sk-ant-oat01-...`. Save it — you'll put it on the server at `/var/secrets/claude/oauth_token`.

**How it works at runtime:**
- The dashboard backend reads `/var/secrets/claude/oauth_token` on the host
- When running Claude inside an agent container via SSH, it passes the token as `CLAUDE_CODE_OAUTH_TOKEN` env var in the command
- The same token is used for the auto-classify feature (runs on the host)
- No credential files (`.credentials.json`, `.claude.json`) are needed

**When the token expires (after ~1 year):**
- Run `claude setup-token` again locally
- Update `/var/secrets/claude/oauth_token` on the server
- No restart needed — the dashboard reads the file on each task

### 5. SSH Commit Signing Key

Agent containers sign git commits with an SSH key so they can push to repos with signed-commit requirements.

1. Generate the key on the server:
   ```bash
   mkdir -p /var/secrets/claude
   ssh-keygen -t ed25519 -f /var/secrets/claude/signing_key -N "" -C "claude-dashboard-signing"
   ```

2. Add the **public key** to GitHub as a **Signing Key**:
   - Go to https://github.com/settings/keys
   - Click "New SSH key"
   - Key type: **Signing Key**
   - Paste the contents of `/var/secrets/claude/signing_key.pub`

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
  claude/
    oauth_token      # Long-lived Claude OAuth token (from `claude setup-token`)
    signing_key      # SSH signing key for git commits
    signing_key.pub  # Public key (add to GitHub as signing key)
  github-app.pem     # GitHub App private key (if using App auth)

/var/lib/dashboard/
  dashboard.db       # SQLite database (created automatically)

/var/lib/claude-agents/
  .next_slot         # Monotonic IP allocation counter (shared with previews)
  .system-path       # Cached agent system closure store path
  a-XXXXXX           # Tracking files for active agents (slot host_ip container_ip)

/var/lib/preview-deploys/
  .system-path       # Cached node preview system closure store path
  .vertex-system-path # Cached vertex preview system closure store path
  t-XXXXXX           # Tracking files for active previews
  t-XXXXXX.type      # Preview type (node or vertex)

/etc/nixos/
  configuration.nix  # Main NixOS config (with real values, NOT the template)
  agent-config.nix   # Agent container NixOS config
  agent.sh           # Agent lifecycle script
  preview-config.nix # Node.js preview container config
  vertex-preview-config.nix  # Vertex preview container config
  preview.sh         # Preview lifecycle script
  flake.nix          # Nix flake for building container closures

/etc/caddy/previews/
  t-XXXXXX.caddy     # Auto-generated Caddy route files per preview
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
CHROMIUM_BIN=/run/current-system/sw/bin/chromium
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

**`/var/secrets/claude/oauth_token`:**

```
sk-ant-oat01-XXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX
```

Just the raw token string, no newlines or quotes.

### NixOS Configuration

The `configuration.nix` file includes:
- **Dashboard systemd service** — runs `/opt/dashboard/dashboard` with `EnvironmentFile=/var/secrets/dashboard.env`
- **Preview webhook systemd service** — runs the Node.js webhook
- **Caddy reverse proxy** — routes `dashboard.yourdomain.com` to port 3200, imports per-preview Caddy configs
- **NAT** — enables container internet access via `ve-+` interfaces
- **PostgreSQL** — shared database for preview containers
- **`agent` and `preview` CLI tools** — wrapped shell scripts for container management

**Important:** `configuration.nix` in this repo has placeholder values (`YOUR.SERVER.IP.HERE`, `ssh-ed25519 AAAA... your-key-here`, etc.). The setup script substitutes these with real values. **Never scp the template directly to the server.**

### First-time Setup

Use the `setup.sh` script which handles:
1. Substituting placeholder values in configuration files
2. Creating secrets directories and environment files
3. Uploading all NixOS configs
4. Running `nixos-rebuild switch`
5. Building agent and preview container closures
6. Building and deploying the dashboard binary and frontend

```bash
./setup.sh           # Basic setup
./setup.sh --vertex  # With Vertex (Elixir/Phoenix) preview support
```

After setup completes, you still need to:
1. Generate and upload the Claude OAuth token (see section 4 above)
2. Deploy the dashboard (see below)

## Deploying Changes

Use the `deploy.sh` script from the repo root. It pushes your current branch, pulls it on the server, and builds everything remotely using `nix-shell` — no local builds required.

```bash
./deploy.sh <server-ip>                    # Deploy everything
./deploy.sh <server-ip> dashboard          # Dashboard only (backend + frontend)
./deploy.sh <server-ip> webhook            # Preview webhook only
./deploy.sh <server-ip> nix                # NixOS configs only (nixos-rebuild)
./deploy.sh <server-ip> nix-agents         # NixOS configs + rebuild agent closure
./deploy.sh <server-ip> nix-previews       # NixOS configs + rebuild preview closures
```

**How it works:**
1. Ensures a git clone exists at `/opt/src/` on the server (creates on first run)
2. Pushes your current branch to origin and pulls it on the server
3. Builds frontend with `nix-shell -p nodejs_22 --run 'npm ci && npm run build'`
4. Builds backend with `nix-shell -p rustc cargo gcc pkg-config --run 'cargo build --release'`
5. Swaps binaries/static files atomically (stop service → swap → start service)

### What each component does

- **`dashboard`** — Builds frontend (Node.js) and backend (Rust), copies them to `/opt/dashboard/`, restarts the service
- **`webhook`** — Copies webhook source to `/opt/preview-webhook/`, runs `npm ci && npm run build`, restarts the service
- **`nix`** — Copies NixOS config files (agent-config.nix, preview-config.nix, etc.) to `/etc/nixos/`, runs `nixos-rebuild switch`
- **`nix-agents`** — Same as `nix`, plus rebuilds the agent container closure (`agent build`)
- **`nix-previews`** — Same as `nix`, plus rebuilds both preview closures (`preview build && preview build --type vertex`)

### Notes

- **Never scp `configuration.nix` directly** — the local copy has placeholder values. Either edit on the server or use `setup.sh` for a fresh install.
- The deploy script skips `configuration.nix` automatically (it only copies the supporting `.nix` files and scripts).
- First Rust build on a fresh server takes a while (downloading crates). Subsequent builds are fast thanks to cached target directory.

### Claude OAuth token refresh

When the token expires (after ~1 year):

```bash
# On your local machine:
claude setup-token
# Copy the token

# On the server:
ssh root@<server> 'echo "sk-ant-oat01-XXXXX" > /var/secrets/claude/oauth_token && chmod 600 /var/secrets/claude/oauth_token'
```

No service restart needed — the dashboard reads the file on each task.

## Deploying from Scratch (Complete Steps)

Here is every step to go from a bare Hetzner server to a fully working deployment:

### Phase 1: Server + NixOS

1. **Order a Hetzner dedicated server** (e.g., AX41-NVMe)
2. **Activate rescue mode**: Hetzner Robot > Server > Rescue tab > Activate Linux 64-bit
3. **Hardware reset**: Reset tab > Hardware reset
4. **Wait ~1 min**, verify SSH: `ssh root@YOUR_SERVER_IP`
5. **Run setup script**:
   ```bash
   git clone <this-repo>
   cd nixos-claude_3
   ./setup.sh --vertex
   ```
   This installs NixOS, copies all configs, builds container closures. Takes 10-20 minutes.

### Phase 2: Cloudflare

6. **Add domain to Cloudflare** (or use existing)
7. **Set SSL/TLS mode to Full (strict)**
8. **Generate Origin CA certificate** (SSL/TLS > Origin Server > Create Certificate)
   - Hostnames: `*.yourdomain.com, yourdomain.com`
   - Save cert and key as `.pem` files
9. **Upload certs to server**:
   ```bash
   scp cloudflare-origin.pem root@<server>:/var/secrets/cloudflare-origin.pem
   scp cloudflare-origin-key.pem root@<server>:/var/secrets/cloudflare-origin-key.pem
   ```
10. **Add DNS records** (all proxied):
    - `A` `dashboard` → `<server-ip>`
    - `A` `*` → `<server-ip>`
    - `A` `@` → `<server-ip>`

### Phase 3: GitHub App

11. **Create GitHub App** (see section 2 above)
12. **Upload private key**:
    ```bash
    scp github-app.pem root@<server>:/var/secrets/github-app.pem
    ```
13. **Configure GitHub webhook** on each repo:
    - URL: `https://dashboard.yourdomain.com/api/webhooks/github`
    - Secret: your webhook secret
    - Events: Pull requests

### Phase 4: Google OAuth

14. **Create OAuth credentials** in Google Cloud Console (see section 3 above)

### Phase 5: Secrets

15. **Create environment files on the server**:
    ```bash
    ssh root@<server>
    # Edit /var/secrets/dashboard.env with all variables (see template above)
    # Edit /var/secrets/preview.env with all variables (see template above)
    ```

16. **Generate Claude OAuth token** (on your local machine):
    ```bash
    claude setup-token
    ```
    ```bash
    ssh root@<server> 'echo "sk-ant-oat01-XXXXX" > /var/secrets/claude/oauth_token && chmod 600 /var/secrets/claude/oauth_token'
    ```

17. **Generate SSH signing key** (on the server):
    ```bash
    ssh root@<server>
    ssh-keygen -t ed25519 -f /var/secrets/claude/signing_key -N "" -C "claude-dashboard-signing"
    ```
    Then add the public key (`/var/secrets/claude/signing_key.pub`) to GitHub as a **Signing Key** at https://github.com/settings/keys.

### Phase 6: Deploy Dashboard + Webhook

18. **Commit and push your code**, then deploy everything with the deploy script:
    ```bash
    git push origin main
    ./deploy.sh <server-ip>
    ```
    This pushes your branch, pulls on the server, builds frontend + backend + webhook remotely using `nix-shell`, and restarts all services. No local builds needed.

    For component-specific deploys:
    ```bash
    ./deploy.sh <server-ip> dashboard     # Just the dashboard
    ./deploy.sh <server-ip> webhook       # Just the webhook
    ```

### Phase 7: Verify

19. **Check everything**:
    ```bash
    ssh root@<server>

    # Services running
    systemctl status dashboard
    systemctl status preview-webhook
    systemctl status caddy

    # Dashboard accessible
    curl -s https://dashboard.yourdomain.com | head -5

    # Agent containers work
    agent create test
    agent list
    agent destroy test

    # Preview containers work
    preview build
    preview build --type vertex  # if using vertex
    ```

20. **Create a test task** via the dashboard UI to verify the full pipeline:
    - Claude streaming logs should show tool use (⚡ Reading, ✏️ Editing, etc.)
    - Preview URL should appear after first Claude run
    - Iframe should show the live preview
    - Follow-up chat should work
    - "Mark Done" should complete the task

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

### Claude fails with "OAuth token has expired"
The token in `/var/secrets/claude/oauth_token` has expired. Generate a new one:
```bash
# On your local machine:
claude setup-token
# Then update on server:
ssh root@<server> 'echo "NEW_TOKEN" > /var/secrets/claude/oauth_token'
```
No restart needed.

### Claude fails with "Failed to read Claude OAuth token"
The file `/var/secrets/claude/oauth_token` doesn't exist on the server. Create it (see step 16 above).

### Push rejected (signed commits required)
- Verify the signing key is in `/var/secrets/claude/signing_key`
- Verify the public key is added to GitHub as a **Signing Key** (not Authentication Key)
- The commit email in `agent-config.nix` must match the GitHub account that has the signing key

### Preview shows 502 Bad Gateway
The app inside the container hasn't started yet (especially vertex previews which take several minutes to build):
```bash
preview logs <slug> --follow
nixos-container run <slug> -- systemctl status setup-vertex   # or setup-preview
nixos-container run <slug> -- journalctl -u setup-vertex -n 30
```

### Preview shows old code after task completes
For vertex previews, the build takes 1-3 minutes after `preview create` or `preview update`. The preview URL works immediately but serves stale assets until the build finishes. Check build progress:
```bash
preview logs <slug> --follow
```

### Binary can't be replaced ("Text file busy")
The dashboard process is running. Stop it first:
```bash
systemctl stop dashboard
cp /path/to/new/dashboard /opt/dashboard/dashboard
systemctl start dashboard
```
