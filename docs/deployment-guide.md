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
- [GitHub Personal Access Token](https://github.com/settings/tokens) with `repo` scope
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

### 2. GitHub Personal Access Token

Create a Personal Access Token (classic) at [GitHub Settings > Developer settings > Personal access tokens > Tokens (classic)](https://github.com/settings/tokens):

1. Click **Generate new token** > **Generate new token (classic)**
2. Give it a descriptive name (e.g., "Preview Deployments")
3. Select the **`repo`** scope (full control of private repositories)
4. Click **Generate token**
5. **Copy the token** — it's only shown once

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
- Asks for optional secret file paths to upload (Cloudflare cert + key, GitHub PAT)
- Shows a summary and asks for confirmation

With `--skip-install`, rescue-mode hardware detection is skipped (the server is already running NixOS).

**Phase 2: Install NixOS**
- Copies `initial-install/` to a temp directory, substitutes detected hardware values
- Runs [nixos-anywhere](https://github.com/nix-community/nixos-anywhere) to install NixOS remotely (takes 5-10 minutes)
- Waits for the server to reboot and come back online

Skipped with `--skip-install`.

**Phase 3: Server setup**
- Clones this repository on the server at `/opt/src/`
- Uploads secret files to `/var/secrets/` (Cloudflare certs, GitHub PAT)
- SSHes into the server and runs `server-setup.sh` interactively

#### What server-setup.sh does (runs on the server)

The `server-setup.sh` script is the main server-side configuration tool. It prompts for all configuration values and handles the full build:

1. **Configuration prompts**: domain, GitHub OAuth App credentials, GitHub organization, GitHub PAT, webhook secret, allowed repos, vertex repos, SSH public key
2. **Hardware detection**: auto-detects server IP, gateway, interface, disks, and kernel modules from the running system
3. **Secret generation**: JWT secret, root SSH key
4. **NixOS configuration**: substitutes all placeholders in `.nix` config files and installs to `/etc/nixos/`
5. **Environment files**: writes `dashboard.env`, `preview.env`, and Caddy webhook route
6. **NixOS rebuild**: runs `nixos-rebuild switch` (handles Caddy hash auto-fix on first build)
7. **Cloudflare cert permissions**: fixes cert ownership for Caddy
8. **Dashboard build**: builds frontend (React) and backend (Rust) from the repo
9. **Webhook build**: builds the preview webhook service (TypeScript)
10. **Container closures**: pre-builds agent, preview, and vertex container closures
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
3. Builds the requested components using `nix-shell` on the server
4. Restarts affected services

### What each component does

- **`dashboard`** — Builds frontend (Node.js) and backend (Rust) from `/opt/src/dashboard/`, deploys to `/opt/dashboard/`, restarts the service
- **`webhook`** — Copies webhook source to `/opt/preview-webhook/`, runs `npm ci && npm run build`, restarts the service
- **`nix`** — Copies safe NixOS config files (not `configuration.nix` or `agent-config.nix`) to `/etc/nixos/`, runs `nixos-rebuild switch`
- **`nix-agents`** — Same as `nix`, plus rebuilds the agent container closure (`agent build`)
- **`nix-previews`** — Same as `nix`, plus rebuilds both preview closures (`preview build && preview build --type vertex`)

### Important notes

- **Never scp `configuration.nix` or `agent-config.nix` directly** — the local copies have placeholder values. Either edit on the server or use `server-setup.sh` for a full reconfigure.
- The deploy script automatically skips both template files.
- First Rust build on a fresh server takes a while (downloading crates). Subsequent builds are fast.

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
              - Rust/Axum backend        - Node.js or Vertex (Elixir)
              - React SPA                - One per PR/branch
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
  github-pat                            # GitHub Personal Access Token
  claude/
    oauth_token                        # Long-lived Claude OAuth token

/var/lib/dashboard/
  dashboard.db                         # SQLite database (created automatically)

/var/lib/claude-agents/
  .next_slot                           # Monotonic IP allocation counter (shared)
  .system-path                         # Cached agent system closure store path
  a-XXXXXX                             # Tracking files for active agents

/var/lib/preview-deploys/
  .system-path                         # Cached node preview closure store path
  .vertex-system-path                  # Cached vertex preview closure store path
  t-XXXXXX                             # Tracking files for active previews
  t-XXXXXX.type                        # Preview type (node or vertex)

/etc/nixos/
  configuration.nix                    # Main NixOS config (with real values)
  agent-config.nix                     # Agent container NixOS config
  agent.sh                             # Agent lifecycle script
  preview-config.nix                   # Node.js preview container config
  vertex-preview-config.nix            # Vertex preview container config
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
VERTEX_REPOS=yourorg/elixir-repo
PREVIEW_BIN=/run/current-system/sw/bin/preview
AGENT_BIN=/run/current-system/sw/bin/agent
STATIC_DIR=/opt/dashboard/static
```

**`/var/secrets/preview.env`:**

```bash
GITHUB_TOKEN=<personal-access-token>
GITHUB_WEBHOOK_SECRET=<webhook-secret>
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
| Preview returns 502 | App is still building. Check `preview logs <slug> --follow`. Vertex builds take 10-15 minutes. |
| Container fails to start | `journalctl -u container@<name>` on host. |

### Authentication issues

| Problem | Solution |
|---------|----------|
| Claude "Invalid API key" | Re-run `claude login` on host, fix permissions, recreate container. |
| Claude "OAuth token has expired" | Generate a new token: `claude setup-token` (local), then update `/var/secrets/claude/oauth_token` on server. |
| Claude "Failed to read Claude OAuth token" | File `/var/secrets/claude/oauth_token` doesn't exist. Create it (see Claude OAuth section above). |
| SSH asks for password | SSH key not baked into container config. Run `agent build` after config changes, then recreate. |
| SSH host key warning | `ssh-keygen -R <container-ip>` |
| Push rejected / commit not verified | Commits are created via the GitHub GraphQL API using each user's OAuth token. Verify the user's token has `repo` scope (re-login to the dashboard to refresh it). |

### Network issues

| Problem | Solution |
|---------|----------|
| Containers can't reach internet | Check NAT: `sysctl net.ipv4.ip_forward` (should be 1), `iptables -t nat -L POSTROUTING` (should show MASQUERADE). |
| Browser shows `NET::ERR_CERT_AUTHORITY_INVALID` | Cloudflare SSL/TLS mode must be **Full (strict)**. DNS records must be **Proxied** (orange cloud). Check DNS propagation with `dig`. |
| Database connection issues (node previews) | Verify PostgreSQL: `systemctl status postgresql`. Verify firewall trusts container interfaces: `networking.firewall.trustedInterfaces = [ "ve-+" ]` in `configuration.nix`. |

### Preview-specific issues

```bash
# Re-trigger a build inside a container
nixos-container run <slug> -- systemctl restart setup-preview  # node
nixos-container run <slug> -- systemctl restart setup-vertex   # vertex

# Clean up a stuck preview manually
nixos-container stop <slug> 2>/dev/null
nixos-container destroy <slug>
rm -f /var/lib/preview-deploys/<slug> /var/lib/preview-deploys/<slug>.type
rm -f /etc/caddy/previews/<slug>.caddy
systemctl reload caddy
# For node type only:
sudo -u postgres psql -c "DROP DATABASE IF EXISTS preview_<slug_underscored>;"
sudo -u postgres psql -c "DROP USER IF EXISTS preview_<slug_underscored>;"
```

### Vertex secrets (optional)

If using Vertex previews with apps that need additional credentials, add to `/var/secrets/preview.env`:

```bash
VERTEX_POSTMARK_API_KEY=your-key
VERTEX_GOOGLE_CLIENT_ID=your-client-id
VERTEX_GOOGLE_CLIENT_SECRET=your-client-secret
```

Then restart the webhook: `systemctl restart preview-webhook`
