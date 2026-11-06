# Deployment Guide

Complete guide to deploying Tekton from scratch and maintaining it afterwards.

## Prerequisites

**Local machine:**
- [Nix installed](https://nixos.org/download/) with flakes enabled
- SSH key pair (`ssh-keygen -t ed25519` if you don't have one)
- Git

**Server:**
- A Hetzner dedicated server (e.g., AX41-NVMe) with at least 2 disks

**External services (set up before or during install):**
- [Cloudflare](https://dash.cloudflare.com) account with your domain added
- A private [GitHub App](#2-github-app) owned by your org, for the preview webhook
- [GitHub OAuth App](https://github.com/settings/developers) for dashboard login
- Claude Max/Teams subscription

## External Services Setup

### 1. Cloudflare DNS + Origin CA

#### Add your domain to Cloudflare

1. Create a [Cloudflare account](https://dash.cloudflare.com/sign-up) if you don't have one
2. Click **Add a site** and enter your domain (e.g., `example.com`)
3. Select the **Free** plan
4. Cloudflare will scan your existing DNS records — review and confirm
5. Cloudflare will give you two nameservers (e.g., `anna.ns.cloudflare.com`, `bob.ns.cloudflare.com`)
6. Go to your domain registrar and **replace your current nameservers** with the Cloudflare ones
7. Back in Cloudflare, click **Done, check nameservers**

Nameserver propagation can take up to 24 hours, but usually completes within minutes. Cloudflare will email you when the domain is active.

#### Set SSL/TLS mode to Full (strict)

**Critical** — without this, preview URLs will show `NET::ERR_CERT_AUTHORITY_INVALID` in the browser.

1. Go to **Cloudflare Dashboard > your domain > SSL/TLS > Overview**
2. Set the encryption mode to **Full (strict)**
3. Under Edge Certificates, enable **Always Use HTTPS**

Origin CA certificates are only trusted by Cloudflare's proxy, not by browsers directly. "Full (strict)" ensures Cloudflare validates the Origin CA cert on the server and presents its own trusted certificate to browsers.

#### Generate an Origin CA certificate

1. Go to **Cloudflare Dashboard > your domain > SSL/TLS > Origin Server**
2. Click **Create Certificate**
3. Keep the default key type (RSA)
4. Set hostnames to: `*.yourdomain.com` and `yourdomain.com`
5. Choose validity period (15 years recommended)
6. Click **Create**
7. **Important**: You will see two text boxes — the **Origin Certificate** and the **Private Key**. Copy each one and save them as separate `.pem` files (e.g., `cloudflare-origin.pem` and `cloudflare-origin-key.pem`). **The private key is only shown once** — if you lose it, you'll need to generate a new certificate.

The setup script will prompt for these file paths and upload them to the server.

#### Configure DNS records

In Cloudflare DNS, add these records (all must be **Proxied** — orange cloud icon):

| Type | Name | Content | Proxy |
|------|------|---------|-------|
| A | `dashboard` | `YOUR_SERVER_IP` | Proxied |
| A | `*` | `YOUR_SERVER_IP` | Proxied |
| A | `@` | `YOUR_SERVER_IP` | Proxied |

The wildcard record covers preview subdomains (e.g., `42.yourdomain.com`) and the webhook subdomain.

**Important**: All records must be **proxied** (orange cloud). If set to "DNS only" (grey cloud), browsers will connect directly to your server and reject the Origin CA certificate.

### 2. GitHub App

The preview webhook authenticates to GitHub as a GitHub App. For each request it mints an installation token that expires after one hour and is limited to one repo and the minimum permission:

- `contents: read` to look up commits, read `preview-config.nix`, and clone inside the preview container;
- `pull_requests: write` to add the preview link to the PR description.

Personal Access Tokens are not supported. The webhook refuses to start if `GITHUB_TOKEN` is set in `preview.env`.

**Register the App** at **Org settings > Developer settings > GitHub Apps > New GitHub App** (`https://github.com/organizations/<org>/settings/apps/new`):

1. **GitHub App name**: e.g., "Tekton"
2. **Homepage URL**: `https://dashboard.yourdomain.com`
3. **Webhook**: uncheck **Active**. Previews are triggered by the per-repo webhooks (see [Post-setup](#step-3-post-setup)), not by App events.
4. **Repository permissions**: set **Contents** to *Read-only* and **Pull requests** to *Read and write*. **Metadata** becomes *Read-only* automatically. Leave everything else at *No access*.
5. **Where can this GitHub App be installed?**: **Only on this account**. A private App can't be installed by any other account.
6. Click **Create GitHub App**.
7. Note the **App ID** shown under **About** on the App's settings page.
8. Under **Private keys**, click **Generate a private key**. A `.pem` file downloads. This is the most sensitive secret in the deployment: it can mint tokens for every repo the App is installed on. Keep it out of shared drives and chat, and delete your local copy once it's uploaded.

**Install the App**: on the App's page, click **Install App**, choose your org, select **Only select repositories**, and pick the repos that should get previews. After installing, the browser lands on `https://github.com/organizations/<org>/settings/installations/<installation-id>`. The number at the end is the **installation ID**.

Only repos owned by the installation's account can get tokens. If `ALLOWED_REPOS` lists repos from another owner, their previews will fail.

The setup scripts ask for:
- **Path to the App's private key** (`setup.sh`, local), uploaded to `/var/secrets/github-app.pem`, owned by root, mode `0400`
- **App ID** and **installation ID** (`server-setup.sh`), written to `preview.env`

**Rotating the key**: generate a new private key on the App's settings page, replace `/var/secrets/github-app.pem` (root, `0400`), run `systemctl restart preview-webhook`, check that a preview still gets created, then delete the old key on the App's settings page.

### 3. GitHub OAuth App

The dashboard uses GitHub OAuth for login. Users must be members of a specified GitHub organization to access it. Their OAuth token (with `repo` scope) is also used to create verified (signed) commits via the GitHub GraphQL API.

1. Go to [GitHub Settings > Developer settings > OAuth Apps](https://github.com/settings/developers)
2. Click **New OAuth App**
3. Fill in:
   - **Application name**: e.g., "Preview Dashboard"
   - **Homepage URL**: `https://dashboard.yourdomain.com`
   - **Authorization callback URL**: `https://dashboard.yourdomain.com/api/auth/callback`
4. Click **Register application**
5. Note the **Client ID** (shown on the app page)
6. Click **Generate a new client secret** and **copy it immediately** — it's only shown once

The setup script will prompt for:
- **GitHub OAuth App Client ID** — the Client ID from step 5
- **GitHub OAuth App Client Secret** — the secret from step 6
- **GitHub organization** — only members of this org can log in (e.g., `lambdaclass`)

**Important**: The OAuth App requests `repo` and `read:org` scopes. The `repo` scope is required so the dashboard can create branches and verified commits on behalf of each user via the GitHub API. The `read:org` scope is used to verify org membership.

## First-Time Setup

### Step 1: Prepare the Hetzner server

1. Order a dedicated server at [Hetzner Robot](https://robot.hetzner.com)
2. Wait for provisioning and note your server IP
3. Activate rescue mode: **Server > Rescue tab > Activate Linux 64-bit**
4. Reset the server: **Reset tab > Hardware reset**
5. Wait ~1 minute for the server to boot into rescue mode
6. Verify you can SSH in: `ssh root@YOUR_SERVER_IP`

### Step 2: Run setup.sh

```bash
./setup.sh
```

Or, if NixOS is already installed (e.g., re-running after a failed setup):

```bash
./setup.sh --skip-install
```

#### What setup.sh does

The script runs through 3 phases:

**Phase 1: Gather information (local)**
- Prompts for your server IP and selects your SSH key
- SSHes into rescue mode to auto-detect: gateway IP, network interface (translates rescue `eth0` to predictable name like `enp3s0`), prefix length, disk devices, and kernel modules
- Asks for secret file paths to upload (Cloudflare cert + key, GitHub App private key)
- Shows a summary and asks for confirmation

With `--skip-install`, rescue-mode hardware detection is skipped (the server is already running NixOS).

**Phase 2: Install NixOS**
- Copies `initial-install/` to a temp directory, substitutes detected hardware values
- Runs [nixos-anywhere](https://github.com/nix-community/nixos-anywhere) to install NixOS remotely (takes 5-10 minutes)
- Waits for the server to reboot and come back online

Skipped with `--skip-install`.

**Phase 3: Server setup**
- Clones this repository on the server at `/opt/src/`
- Uploads secret files to `/var/secrets/` (Cloudflare certs, GitHub App private key as `github-app.pem`, root `0400`)
- SSHes into the server and runs `server-setup.sh` interactively

#### What server-setup.sh does (runs on the server)

The `server-setup.sh` script is the main server-side configuration tool. It prompts for all configuration values and handles the full build:

1. **Configuration prompts**: domain, GitHub OAuth App credentials, GitHub organization, webhook secret, GitHub App ID and installation ID, allowed repos, SSH public key
2. **Hardware detection**: auto-detects server IP, gateway, interface, disks, and kernel modules from the running system
3. **Secret generation**: JWT secret, root SSH key
4. **NixOS configuration**: substitutes all placeholders in `.nix` config files and installs to `/etc/nixos/`
5. **Environment files**: writes `dashboard.env`, `preview.env`, and Caddy webhook route
6. **NixOS rebuild**: runs `nixos-rebuild switch` (handles Caddy hash auto-fix on first build)
7. **Cloudflare cert permissions**: fixes cert ownership for Caddy
8. **Dashboard build**: builds frontend (React) and backend (Rust) from the repo
9. **Webhook build**: builds the preview webhook service (TypeScript)
10. **Container closures**: pre-builds agent and preview container closures
11. **Service startup**: starts dashboard and webhook services, verifies they're running
12. **Claude login**: interactive Claude Code authentication

You can also run `server-setup.sh` standalone (SSH into the server and run `cd /opt/src && ./server-setup.sh`).

### Step 3: Post-setup

After setup completes, you'll see a summary with:
- Dashboard URL
- Webhook URL and secret (for GitHub webhook config)
- DNS records to add
- Quick start commands

**Configure the GitHub webhook** on each repo:
1. Go to **Settings > Webhooks > Add webhook**
2. **Payload URL**: `https://webhook.yourdomain.com/webhook/github`
3. **Content type**: `application/json`
4. **Secret**: The webhook secret from setup
5. **Events**: Select "Pull requests" only

## Deploying Changes

After making changes to the codebase, use `deploy.sh` to update the server. It pushes your current branch, pulls it on the server, and builds everything remotely — no local builds needed.

```bash
./deploy.sh <server-ip>                    # Deploy everything
./deploy.sh <server-ip> dashboard          # Dashboard only (backend + frontend)
./deploy.sh <server-ip> webhook            # Preview webhook only
./deploy.sh <server-ip> nix                # NixOS configs only (nixos-rebuild)
./deploy.sh <server-ip> nix-agents         # NixOS configs + rebuild agent closure
./deploy.sh <server-ip> nix-previews       # NixOS configs + rebuild preview closures
```

### How deploy.sh works

1. Ensures a git clone exists at `/opt/src/` on the server (creates on first run)
2. Pushes your current branch to origin and pulls it on the server
3. Checks the GitHub App configuration (`preflight_github_app`): `preview.env` must not set `GITHUB_TOKEN` and must set the three `GITHUB_APP_*` variables, and `/var/secrets/github-app.pem` must exist, owned by root, mode `0400`. On failure it prints the migration steps and stops before anything is installed. It only checks names, paths and modes, never values.
4. Builds the requested components using `nix-shell` on the server
5. Restarts affected services

### What each component does

- **`dashboard`** — Builds frontend (Node.js) and backend (Rust) from `/opt/src/dashboard/`, deploys to `/opt/dashboard/`, restarts the service
- **`webhook`** — Copies webhook source to `/opt/preview-webhook/`, runs `npm ci && npm run build`, restarts the service
- **`nix`** — Copies safe NixOS config files (not `configuration.nix` or `agent-config.nix`) to `/etc/nixos/`, runs `nixos-rebuild switch`
- **`nix-agents`** — Same as `nix`, plus rebuilds the agent container closure (`agent build`)
- **`nix-previews`** — Same as `nix`, plus rebuilds the preview closure (`preview build`)

### Important notes

- **Never scp `configuration.nix` or `agent-config.nix` directly** — the local copies have placeholder values. Either edit on the server or use `server-setup.sh` for a full reconfigure.
- The deploy script automatically skips both template files.
- First Rust build on a fresh server takes a while (downloading crates). Subsequent builds are fast.

### Migrating from a PAT

Servers set up before the GitHub App change have a Personal Access Token as `GITHUB_TOKEN` in `/var/secrets/preview.env`. `deploy.sh` refuses to deploy until it's replaced:

1. **Create and install the App** as described in [GitHub App](#2-github-app), and note the App ID and installation ID.
2. **Upload the key** to the server:
   ```bash
   scp tekton-app.private-key.pem root@<server-ip>:/var/secrets/github-app.pem
   ssh root@<server-ip> 'chown root:root /var/secrets/github-app.pem && chmod 0400 /var/secrets/github-app.pem'
   ```
3. **Edit `/var/secrets/preview.env`** on the server: delete the `GITHUB_TOKEN=` line and add:
   ```bash
   GITHUB_APP_ID=<app-id>
   GITHUB_APP_INSTALLATION_ID=<installation-id>
   GITHUB_APP_PRIVATE_KEY_PATH=/var/secrets/github-app.pem
   ```
4. **Deploy**: `./deploy.sh <server-ip>`. The preflight should pass and `systemctl status preview-webhook` should show it running with `Auth mode: GitHub App` in its log.
5. **Check it works**: open (or close and reopen) a PR on an installed repo, confirm the preview gets created, and confirm the PR description gets its preview link. The edit now shows the App (e.g. `tekton[bot]`) as the editor.
6. **Revoke the PAT** on GitHub (**Settings > Developer settings > Personal access tokens**). Nothing on the server uses it anymore.

### Claude OAuth token refresh

Claude Code authenticates via a long-lived OAuth token (valid for ~1 year) tied to your Claude Max/Teams subscription.

**Generate the token** (on your local machine):
```bash
claude setup-token
```

**Upload to server:**
```bash
ssh root@<server> 'echo "sk-ant-oat01-XXXXX" > /var/secrets/claude/oauth_token && chmod 600 /var/secrets/claude/oauth_token'
```

No service restart needed — the dashboard reads the file on each task.

**How it works at runtime:**
- The dashboard backend reads `/var/secrets/claude/oauth_token` on the host
- When running Claude inside an agent container, the token is passed as `CLAUDE_CODE_OAUTH_TOKEN` env var
- The same token is used for the auto-classify feature
- No credential files (`.credentials.json`, `.claude.json`) are needed

## Server Reference

### Architecture overview

```
                         Cloudflare (DNS + proxy)
                                  |
                           Caddy (TLS termination)
                          /                \
            dashboard.yourdomain.com    *.yourdomain.com
                     |                          |
              Dashboard (port 3200)      Preview containers
              - Rust/Axum backend        - One per PR/branch
              - React SPA                - Config from repo's preview-config.nix
              - SQLite DB
                     |
              Agent containers (a-XXXXXX)
              - Claude Code (auth via CLAUDE_CODE_OAUTH_TOKEN)
              - Git operations (verified commits via GitHub GraphQL API)
              - Created per task, destroyed after completion
```

### Task pipeline

When a user submits a task via the dashboard:

```
1. Create agent container (a-{short_id})
2. Clone repo, create branch claude/{short_id}
3. Run Claude (stream-json) -> commit locally -> push via GitHub API -> create preview (t-{short_id}) -> screenshot
4. Follow-up loop:
   a. Status -> awaiting_followup
   b. Wait for message (poll 10s)
   c. If message: run Claude -> commit -> push via API -> update preview -> screenshot -> repeat
   d. If __done__: break
5. Destroy agent container -> status: completed
```

Key points:
- Agent and preview containers have different prefixes (`a-` for agents, `t-` for previews)
- Push + preview happen after each Claude run, not just at the end
- Pushes use the GitHub GraphQL `createCommitOnBranch` mutation, producing verified (signed) commits attributed to the logged-in user
- Claude's actions are streamed via `--output-format stream-json --verbose`
- Claude authenticates via `CLAUDE_CODE_OAUTH_TOKEN` env var

### Directory structure

```
/opt/src/                              # Git clone of this repo (setup.sh clones it)

/opt/dashboard/
  dashboard                            # Rust binary
  static/                              # Frontend build (index.html, assets/)

/opt/preview-webhook/
  dist/                                # Compiled TypeScript
  node_modules/
  package.json

/var/secrets/
  dashboard.env                        # Dashboard environment variables
  preview.env                          # Preview webhook environment variables
  cloudflare-origin.pem                # Cloudflare Origin CA cert
  cloudflare-origin-key.pem            # Cloudflare Origin CA key
  github-app.pem                       # GitHub App private key (root, 0400)
  claude/
    oauth_token                        # Long-lived Claude OAuth token

/var/lib/dashboard/
  dashboard.db                         # SQLite database (created automatically)

/var/lib/claude-agents/
  .next_slot                           # Monotonic IP allocation counter (shared)
  .system-path                         # Cached agent system closure store path
  a-XXXXXX                             # Tracking files for active agents

/var/lib/preview-deploys/
  t-XXXXXX                             # Tracking files for active previews
  t-XXXXXX.meta                        # Preview meta JSON (from preview-config.nix)
  t-XXXXXX.sha                         # Commit SHA of the deployed version

/etc/nixos/
  configuration.nix                    # Main NixOS config (with real values)
  agent-config.nix                     # Agent container NixOS config
  agent.sh                             # Agent lifecycle script
  preview-config.nix                   # Default preview container config (Node.js)
  preview.sh                           # Preview lifecycle script
  flake.nix                            # Nix flake for building container closures

/etc/caddy/previews/
  webhook.caddy                        # Webhook Caddy route
  t-XXXXXX.caddy                       # Auto-generated per-preview Caddy routes
```

### Environment files

**`/var/secrets/dashboard.env`:**

```bash
LISTEN_ADDR=0.0.0.0:3200
DATABASE_URL=sqlite:///var/lib/dashboard/dashboard.db
JWT_SECRET=<random-64-char-hex-string>
GITHUB_CLIENT_ID=<from-github-oauth-app>
GITHUB_CLIENT_SECRET=<from-github-oauth-app>
GITHUB_REDIRECT_URI=https://dashboard.yourdomain.com/api/auth/callback
GITHUB_ORG=yourorg
PREVIEW_DOMAIN=yourdomain.com
ALLOWED_REPOS=yourorg/repo1,yourorg/repo2
PREVIEW_BIN=/run/current-system/sw/bin/preview
AGENT_BIN=/run/current-system/sw/bin/agent
STATIC_DIR=/opt/dashboard/static
# Optional: agent container retention (defaults shown)
AGENT_RETENTION_FAILED_HOURS=24
AGENT_RETENTION_PUSH_FAILED_HOURS=168
SNAPSHOT_DIR=/var/lib/claude-agents/snapshots
SNAPSHOT_RETENTION_DAYS=14
```

**`/var/secrets/preview.env`:**

```bash
GITHUB_WEBHOOK_SECRET=<webhook-secret>
PREVIEW_DOMAIN=yourdomain.com
WEBHOOK_PORT=3100
ALLOWED_REPOS=yourorg/repo1,yourorg/repo2
GITHUB_APP_ID=<app-id>
GITHUB_APP_INSTALLATION_ID=<installation-id>
GITHUB_APP_PRIVATE_KEY_PATH=/var/secrets/github-app.pem
ADMIN_SSH_KEY="ssh-ed25519 AAAA..."
```

No `GITHUB_TOKEN`: the webhook refuses to start if it's set, and `preview` refuses to run.

**`/var/secrets/claude/oauth_token`:**

```
sk-ant-oat01-XXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX
```

Just the raw token string, no newlines or quotes.

### NixOS configuration placeholders

Template files in the repo have placeholder values that `server-setup.sh` substitutes with real values during setup. **Never copy them directly to the server.**

| File | Placeholder | Substituted with |
|------|-------------|-----------------|
| `configuration.nix` | `YOUR.SERVER.IP.HERE` | Server IP address |
| `configuration.nix` | `YOUR.GATEWAY.IP.HERE` | Gateway IP address |
| `configuration.nix` | `DISK_DEVICE_0`, `DISK_DEVICE_1` | Disk device paths |
| `configuration.nix` | `INITRD_KERNEL_MODULES` | Initrd kernel modules |
| `configuration.nix` | `dashboard.YOUR_DOMAIN` | Dashboard domain |
| `configuration.nix` | `ssh-ed25519 AAAA... your-key-here` | Your SSH public key |
| `agent-config.nix` | `ssh-ed25519 AAAA... your-key-here` | Your SSH public key |
| `agent-config.nix` | `ssh-ed25519 AAAA... root-key-here` | Server's root SSH public key |

## Troubleshooting

### Setup issues

| Problem | Solution |
|---------|----------|
| Can't SSH into rescue mode | Verify rescue mode is active in Hetzner Robot. Check your SSH key. |
| nixos-anywhere fails | Check DNS in rescue system (`cat /etc/resolv.conf`). The script configures this automatically but it can fail. |
| Server doesn't come back after install | Check the Hetzner console (KVM) for boot errors. May need to re-run setup from rescue mode. |
| Caddy hash error on first build | Handled automatically — the script extracts the correct hash, patches the config, and retries. |

### Service issues

| Problem | Solution |
|---------|----------|
| Dashboard won't start | `journalctl -u dashboard -n 50`. Check `/var/secrets/dashboard.env` exists and has all variables. |
| Webhook not receiving events | `systemctl status preview-webhook` and check GitHub webhook deliveries. |
| Preview returns 502 | App is still building. Check `preview logs <slug> --follow`. Some builds (e.g. Elixir) take 10-15 minutes. |
| Container fails to start | `journalctl -u container@<name>` on host. |

### Authentication issues

| Problem | Solution |
|---------|----------|
| Claude "Invalid API key" | Re-run `claude login` on host, fix permissions, recreate container. |
| Claude "OAuth token has expired" | Generate a new token: `claude setup-token` (local), then update `/var/secrets/claude/oauth_token` on server. |
| Claude "Failed to read Claude OAuth token" | File `/var/secrets/claude/oauth_token` doesn't exist. Create it (see Claude OAuth section above). |
| SSH asks for password | SSH key not baked into container config. Run `agent build` after config changes, then recreate. |
| SSH host key warning | `ssh-keygen -R <container-ip>` |
| Preview webhook won't start: "GITHUB_TOKEN is set" | Remove `GITHUB_TOKEN` from `/var/secrets/preview.env`. See [Migrating from a PAT](#migrating-from-a-pat). |
| `preview create/update`: "Could not get a GitHub App token for owner/repo" | Check `journalctl -u preview-webhook`. HTTP 403 means the repo isn't in `ALLOWED_REPOS` or isn't owned by the App installation's account. HTTP 502 usually means the App isn't installed on that repo or the App ID, installation ID or key is wrong. HTTP 000 means the webhook isn't running. |
| Push rejected / commit not verified | Commits are created via the GitHub GraphQL API using each user's OAuth token. Verify the user's token has `repo` scope (re-login to the dashboard to refresh it). |

### Network issues

| Problem | Solution |
|---------|----------|
| Containers can't reach internet | Check NAT: `sysctl net.ipv4.ip_forward` (should be 1), `iptables -t nat -L POSTROUTING` (should show MASQUERADE). |
| Browser shows `NET::ERR_CERT_AUTHORITY_INVALID` | Cloudflare SSL/TLS mode must be **Full (strict)**. DNS records must be **Proxied** (orange cloud). Check DNS propagation with `dig`. |
| Database connection issues (node previews) | Verify PostgreSQL: `systemctl status postgresql`. Verify firewall trusts container interfaces: `networking.firewall.trustedInterfaces = [ "ve-+" ]` in `configuration.nix`. |

### Preview-specific issues

```bash
# Re-trigger a build inside a container (service name comes from the repo's preview-config.nix)
nixos-container run <slug> -- systemctl restart <setup-service-name>

# Clean up a stuck preview manually
nixos-container stop <slug> 2>/dev/null
nixos-container destroy <slug>
rm -f /var/lib/preview-deploys/<slug> /var/lib/preview-deploys/<slug>.meta /var/lib/preview-deploys/<slug>.sha
rm -f /etc/caddy/previews/<slug>.caddy
systemctl reload caddy
# If the preview used database = "host":
sudo -u postgres psql -c "DROP DATABASE IF EXISTS preview_<slug_underscored>;"
sudo -u postgres psql -c "DROP USER IF EXISTS preview_<slug_underscored>;"
```

### Agent containers and snapshots

A task's agent container is kept after the task stops if it may still hold undelivered work:

- **Push failed** (`push_failed`): kept for `AGENT_RETENTION_PUSH_FAILED_HOURS`. Retry push or a follow-up message tries again.
- **Failed after the agent started working**: kept for `AGENT_RETENTION_FAILED_HOURS`. Reopen continues in the same container.

Before any container is destroyed, the dashboard saves the agent's work to `SNAPSHOT_DIR/<task_id>/<unix time>/`:

- `commits.bundle`: the commits ahead of the base branch (missing if there were none)
- `commits.txt`: their one-line log
- `uncommitted.diff`: uncommitted and untracked changes, as a binary diff

Every 15 minutes the dashboard destroys containers whose retention has run out, destroys `task-*` containers whose task finished more than an hour ago, and deletes snapshots older than `SNAPSHOT_RETENTION_DAYS`.

To free space sooner, use **Discard container** on the task page, or the **Agent containers** section of the Admin page. Both save a snapshot first. To do it by hand on the server:

```bash
agent list
agent destroy task-abc123   # no snapshot is taken this way
```

To recover work from a snapshot into a clone of the repo:

```bash
git fetch /var/lib/claude-agents/snapshots/<task_id>/<time>/commits.bundle HEAD:recovered
git checkout recovered
git apply --binary /var/lib/claude-agents/snapshots/<task_id>/<time>/uncommitted.diff
```

### Host secrets for previews (optional)

If a repo's `preview-config.nix` declares `hostSecrets` (e.g. `hostSecrets = [ "POSTMARK_API_KEY" ];`), add those keys to `/var/secrets/preview.env`:

```bash
POSTMARK_API_KEY=your-key
GOOGLE_CLIENT_ID=your-client-id
```

Tekton forwards any matching keys into the container's `/etc/preview.env` at create time. If a key is missing, tekton warns but does not abort.

## Local Development

You can run the dashboard (frontend + backend) locally for UI and API development. Agent containers, task execution, and preview deployments require the full NixOS server setup and **will not work locally** — those features depend on systemd-nspawn, NixOS containers, and the server's Caddy/Cloudflare networking stack.

### Prerequisites

- **Docker** (install [Docker Desktop](https://docs.docker.com/get-docker/))
- **Rust** (stable toolchain, install via [rustup](https://rustup.rs/))
- **Node.js** (22+)
- A **GitHub OAuth App** for local login (see below)

### Create a GitHub OAuth App

1. Go to [GitHub Settings > Developer settings > OAuth Apps](https://github.com/settings/developers) and click **New OAuth App**
2. Fill in:
   - **Application name**: anything (e.g. "Tekton Local Dev")
   - **Homepage URL**: `http://localhost:5173`
   - **Authorization callback URL**: `http://localhost:5173/api/auth/callback`
3. Click **Register application**
4. Copy the **Client ID** from the app page
5. Click **Generate a new client secret** and copy it (shown once)

You'll also need the name of a GitHub organization you belong to — only members of that org can log in.

**Important**: The callback URL must match exactly. For local development it must be `http://localhost:5173/api/auth/callback` (not `localhost:3200` — the Vite dev server proxies API requests to the backend). If you reuse an existing OAuth App that points to a production URL, login will fail. Create a separate OAuth App for local dev.

### Quick start

```bash
make deps
```

This checks prerequisites, starts a PostgreSQL container via Docker, installs frontend npm packages, and creates `dashboard/backend/.env` with placeholders.

Edit `dashboard/backend/.env` and fill in three values:

```
GITHUB_CLIENT_ID=<your client id>
GITHUB_CLIENT_SECRET=<your client secret>
GITHUB_ORG=<your github org>
```

Then:

```bash
make run
```

This starts the Rust backend (port 3200) and the Vite dev server at `http://localhost:5173` (proxies `/api` to the backend). Ctrl-C stops both. Database tables are auto-created on first startup.

### Other commands

```bash
make run-backend    # Run only the backend
make run-frontend   # Run only the frontend
make stop           # Kill running dev servers (keeps database running)
make down           # Stop everything: dev servers + database container
make clean          # Stop everything and delete database data
```

### What works locally

- Dashboard UI (all pages, navigation, admin panel)
- GitHub OAuth login
- Task list, task detail (viewing existing data)
- Intake board (Kanban view, drag-and-drop status changes)
- Admin settings (intake sources, policy presets, AI settings)
- Cost dashboard, audit log
- API endpoints

### What does NOT work locally

- **Creating and running tasks** — requires agent containers (NixOS + systemd-nspawn)
- **Preview deployments** — requires the preview container system and Caddy reverse proxy
- **Screenshots** — requires Chromium inside agent containers
- **Claude Code execution** — requires the Claude binary and OAuth token inside containers

For testing these features, deploy to a server using `./deploy.sh`.
