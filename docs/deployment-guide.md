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

1. **Configuration prompts**: domain, GitHub OAuth App credentials, GitHub organization, GitHub PAT, webhook secret, allowed repos, SSH public key
2. **Hardware detection**: auto-detects server IP, gateway, interface, disks, and kernel modules from the running system
3. **Secret generation**: JWT secret and secrets encryption key, unless `/run/dashboard-secrets/secrets.env` was already set from 1Password. On a server that already has a dashboard install it refuses to generate them and asks you to set them from 1Password instead. Also generates the root SSH key.
4. **NixOS configuration**: substitutes all placeholders in `.nix` config files and installs to `/etc/nixos/`
5. **Environment files**: writes `dashboard.env`, `preview.env`, the Caddy webhook route, and `/run/dashboard-secrets/secrets.env` (tmpfs) if it isn't there yet. See [Dashboard secrets](#dashboard-secrets): if setup generated that file, save its three values in 1Password before the server reboots.
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
3. For `all` and `dashboard`, it runs `server-config/dashboard-secrets.sh check` before touching anything. The check fails if any of these is true:
   - `/run/dashboard-secrets/secrets.env` is missing, or lacks a non-empty value for one of the three [dashboard secrets](#dashboard-secrets).
   - `/var/secrets/dashboard.env` still defines one of them.
   - An old key file still exists (`/var/lib/dashboard/secrets-key`, or a path named by `SECRETS_ENCRYPTION_KEY_FILE=` in `dashboard.env`).
   - `/etc/nixos/configuration.nix` doesn't list the `/run` file. Before the dashboard is restarted, it also checks that the loaded unit reads the file.

   It checks names and paths only and never prints values.
4. Builds the requested components using `nix-shell` on the server
5. Restarts affected services. After restarting the dashboard, it waits a few seconds and fails if the service isn't active or has restarted in the meantime.

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
  dashboard.env                        # Dashboard environment variables (no secrets; see below)
  preview.env                          # Preview webhook environment variables
  cloudflare-origin.pem                # Cloudflare Origin CA cert
  cloudflare-origin-key.pem            # Cloudflare Origin CA key
  github-pat                            # GitHub Personal Access Token
  claude/
    oauth_token                        # Long-lived Claude OAuth token

/run/dashboard-secrets/                # tmpfs, 0700; wiped on reboot
  secrets.env                          # SECRETS_ENCRYPTION_KEY, JWT_SECRET, GITHUB_CLIENT_SECRET (0600), set from 1Password

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
GITHUB_CLIENT_ID=<from-github-oauth-app>
GITHUB_REDIRECT_URI=https://dashboard.yourdomain.com/api/auth/callback
GITHUB_ORG=yourorg
PREVIEW_DOMAIN=yourdomain.com
ALLOWED_REPOS=yourorg/repo1,yourorg/repo2
PREVIEW_BIN=/run/current-system/sw/bin/preview
AGENT_BIN=/run/current-system/sw/bin/agent
STATIC_DIR=/opt/dashboard/static
```

`JWT_SECRET`, `GITHUB_CLIENT_SECRET` and `SECRETS_ENCRYPTION_KEY` don't go in this file; see [Dashboard secrets](#dashboard-secrets). `deploy.sh` refuses to deploy the dashboard while `dashboard.env` still defines any of them.

### Dashboard secrets

Three dashboard secrets never touch the server's persistent disk:

| Variable | What it protects |
|----------|------------------|
| `SECRETS_ENCRYPTION_KEY` | Encrypts what the dashboard stores for users: GitHub OAuth tokens, repo secrets, AI provider keys and Sentry tokens. |
| `JWT_SECRET` | Signs sessions. Anyone holding it can forge an admin session, and the dashboard then uses every stored token on their behalf. |
| `GITHUB_CLIENT_SECRET` | The GitHub OAuth App secret. |

They live in 1Password. An operator pastes them by hand into `/run/dashboard-secrets/secrets.env` (tmpfs, directory 0700, file 0600, root), and the `dashboard` systemd unit loads that file as a second, required `EnvironmentFile`. The dashboard refuses to start if any of the three is missing or empty.

**What this protects against:** database dumps, backups, disk images and snapshots, and bugs that read files from disk. None of these ever hold the key and the ciphertext together. **What it doesn't:** root on the running server, or anything that can read the dashboard's memory. Both can read the secrets out of `/run` or the process.

**A reboot wipes the file.** After a reboot the dashboard stays down (`journalctl -u dashboard` shows "Failed to load environment files") until someone runs `server-config/dashboard-secrets.sh set <server>` and pastes the three values again. This is intended. Nothing sets them automatically.

#### 1Password item and clipboard

By convention the values live in the `Tekton` vault, item `key-tekton-00`, with three fields named `SECRETS_ENCRYPTION_KEY`, `JWT_SECRET` and `GITHUB_CLIENT_SECRET`. Nothing reads the item automatically. You open it in the 1Password app, copy a field, and paste it where a command asks.

There is deliberately no 1Password CLI (`op`) in this flow. A signed-in `op` session lets any process in that terminal read every vault you can see, including a buggy or rogue agent.

All the commands below use [`server-config/dashboard-secrets.sh`](../server-config/dashboard-secrets.sh), run from the repo root on your machine. They connect to the server as root. `set` and `verify-key` prompt on the server with hidden input (`read -s`), so the values never appear on a command line, in either shell's history, on your machine's disk or on the terminal. `copy` and `copy-live` refuse to write to a terminal; pipe them to `pbcopy`.

Clipboard hygiene:

- In the 1Password app, set the clipboard-clearing option (under **Settings → Security**) to 30 seconds. This only covers values copied from 1Password.
- macOS Universal Clipboard (Handoff) can sync the clipboard to your other Apple devices. Turn off Handoff (System Settings → General → AirDrop & Handoff) or accept that.
- After `copy … | pbcopy` or `copy-live … | pbcopy`, the value stays in the clipboard until you replace it. Clear it by hand once it's pasted: `pbcopy < /dev/null`. On Linux use `xclip -selection clipboard` or `wl-copy` instead of `pbcopy`.

What a value can contain: `set` writes each value double-quoted, escaping `\`, `"`, `` ` `` and `$` with a backslash. systemd's `EnvironmentFile` parser undoes exactly that, so the process gets the pasted bytes unchanged. Spaces, tabs, quotes, `$`, backticks, backslashes and non-ASCII text are all fine. Only newlines and other control characters are refused (and invalid UTF-8, which systemd would drop). The only thing removed is what a paste adds: a trailing carriage return or newline, and the terminal's bracketed-paste markers. Leading and trailing spaces are kept, because they are part of the value. A paste with more than one line is refused and you're asked again. For a new `JWT_SECRET`, a generated password of 40 or more characters is plenty.

#### Setting the secrets

```bash
server-config/dashboard-secrets.sh set <server>
```

This prompts three times on the server. For each field, copy it in the 1Password app and paste at the prompt; echo is off for the whole prompt, so nothing shows. An invalid value is asked again, up to three times.

**The key is checked against the live one.** If a dashboard is running, the pasted `SECRETS_ENCRYPTION_KEY` must have the same SHA-256 as the key it uses; otherwise `set` asks again. When no dashboard is running (after a reboot, or on a fresh install), there is nothing to compare with, and `set` says so. In that case a wrong key makes the dashboard refuse to start, via the startup check below. `set <server> --new-key` skips the comparison. That is only for a real key change, and it makes every stored secret unreadable (see [Losing or changing the key](#losing-or-changing-the-key)).

The values go to a temp file next to `secrets.env`. If a value still fails after three tries, or you press Ctrl-C, or the connection drops, the temp file is deleted and `secrets.env` stays as it was. If the shell is killed outright and the temp file survives, the `deploy.sh` preflight reports the leftover `.secrets.env.*` file. Otherwise the temp file replaces `secrets.env`, and the dashboard is restarted and checked, if its unit exists yet. Run this after every reboot, and after changing a value in 1Password.

#### Fresh install

There are two ways to do it:

- **Set first.** Create the item, generating `SECRETS_ENCRYPTION_KEY` and `JWT_SECRET` with the 1Password password generator and taking `GITHUB_CLIENT_SECRET` from the GitHub OAuth App. Then run `set` before `server-setup.sh`. Setup uses the file and doesn't prompt for the client secret.
- **Let setup generate them.** `server-setup.sh` generates `SECRETS_ENCRYPTION_KEY` and `JWT_SECRET`, prompts for `GITHUB_CLIENT_SECRET`, and writes the `/run` file. Before the server reboots, move each value into the 1Password app, one field at a time:
  ```bash
  server-config/dashboard-secrets.sh copy <server> SECRETS_ENCRYPTION_KEY | pbcopy   # paste into the field
  server-config/dashboard-secrets.sh copy <server> JWT_SECRET | pbcopy
  server-config/dashboard-secrets.sh copy <server> GITHUB_CLIENT_SECRET | pbcopy
  pbcopy < /dev/null
  ```

`server-setup.sh` only generates secrets on a fresh server. If it finds an existing install (`/var/secrets/dashboard.env`, state in `/var/lib/dashboard`, or a `dashboard` database) and no `/run` file, it stops and asks you to run `set`. A new key would make every stored secret unreadable. If the old values really are lost, set `ALLOW_NEW_DASHBOARD_SECRETS=1` to override.

#### Losing or changing the key

There is no key rotation: changing `SECRETS_ENCRYPTION_KEY` on an existing install switches keys without re-encrypting anything, which is the same as losing it. Users have to sign in again to store a fresh GitHub token, and repo secrets, AI keys and Sentry tokens have to be re-entered. Treat the 1Password item as the only copy and keep it backed up.

On startup the dashboard tries to decrypt a sample of the oldest values it has stored. If none of them decrypts, it refuses to start with "SECRETS_ENCRYPTION_KEY does not decrypt any of N stored encrypted value(s)". That almost always means the wrong 1Password item or a pasting mistake. This catches a wrong key before the new build encrypts anything with it, including the legacy GitHub tokens it encrypts at startup. It doesn't cover values that an earlier build already wrote under a wrong key; the `verify-key` step in the upgrade below exists for that. One damaged row among good ones doesn't stop it.

#### Upgrading an existing server

Servers set up before this change keep `JWT_SECRET` and `GITHUB_CLIENT_SECRET` in `/var/secrets/dashboard.env`, and may keep `SECRETS_ENCRYPTION_KEY` there too. Prod's key was set by hand in an unknown format, and everything stored is encrypted with the SHA-256 of its exact bytes. **It can't be changed: whatever ends up in 1Password must match the live key exactly.** Do these steps in order, while the current dashboard is still running: `check-live`, `copy-live`, `verify-key` and `set` all read the values the running process actually uses, not what a file appears to say.

1. **Check that the live values can be entered:**
   ```bash
   server-config/dashboard-secrets.sh check-live <server>
   ```
   For each of the three it prints `OK`, `not set`, or that the value contains a newline or another control character, which `set` can't take. It never prints the values. It also notes values that start or end with a space or tab, which must stay exactly that way in 1Password. If it reports a problem with `SECRETS_ENCRYPTION_KEY`, stop: the upgrade can't carry that key over as is. Don't replace the key; get help instead.
2. **Create the item** (`Tekton` vault, item `key-tekton-00`) with the three fields.
3. **Copy the current key and client secret into it.**
   - **`SECRETS_ENCRYPTION_KEY`: copy the existing key exactly.** Everything already encrypted needs it.
     ```bash
     server-config/dashboard-secrets.sh copy-live <server> SECRETS_ENCRYPTION_KEY | pbcopy   # paste into the field
     ```
     Only if `check-live` said it is not set was nothing ever encrypted. In that case, and only then, generate one in the app.
   - **`GITHUB_CLIENT_SECRET`:** `copy-live <server> GITHUB_CLIENT_SECRET | pbcopy` the same way. Optionally, generate a new client secret in the GitHub OAuth App settings instead, and delete the old one there after the deploy.

   Clear the clipboard afterwards (`pbcopy < /dev/null`).
4. **Generate `JWT_SECRET` in the 1Password app** (password generator, 40+ characters). Rotating it only signs everyone out.
5. **Verify the key before deleting anything:**
   ```bash
   server-config/dashboard-secrets.sh verify-key <server>
   ```
   Paste the key from the app at the hidden prompt. The server compares its SHA-256 with that of the key the running dashboard uses, and prints only OK or MISMATCH. On a mismatch, copy it again with `copy-live` and run `verify-key` again. Don't continue until it says OK.
6. **Set the secrets** (`server-config/dashboard-secrets.sh set <server>`, pasting the three fields). `set` checks the key against the running dashboard's again. The restart is harmless, because the running build doesn't read the file yet.
7. **Add the file to the unit on the server.** `deploy.sh` never copies `configuration.nix`, so edit `/etc/nixos/configuration.nix` on the server and change the dashboard's `EnvironmentFile` to:
   ```nix
   EnvironmentFile = [
     "/var/secrets/dashboard.env"
     "/run/dashboard-secrets/secrets.env"
   ];
   ```
8. **Remove the old lines.** On the server, delete the `SECRETS_ENCRYPTION_KEY`, `JWT_SECRET` and `GITHUB_CLIENT_SECRET` lines (and any `SECRETS_ENCRYPTION_KEY_FILE` line) from `/var/secrets/dashboard.env`.
9. **Deploy** with `./deploy.sh <server>` (all components, so `nixos-rebuild` picks up the unit change). The preflight lists anything left over from steps 7 and 8.

Only earlier builds of this branch wrote a key file (`/var/lib/dashboard/secrets-key`, or the path in `SECRETS_ENCRYPTION_KEY_FILE`). Prod never ran them, so it shouldn't have one. If a server does, `copy-live` reads the key from it. The preflight then refuses to deploy until you delete the file, after `verify-key` says OK.

Backups and snapshots taken before the upgrade still hold the old copies. Rotating `JWT_SECRET` (step 4), and the client secret if you did, makes those copies useless. The encryption key can't be rotated, so keep those old backups as protected as the 1Password item.

> **Upgrading from a version before #184:** older versions stored GitHub tokens in plaintext. The first start after upgrading encrypts them, but database backups, dumps and WAL archives from before then still hold live tokens. After the first start, run `VACUUM FULL users;` so the old row versions are dropped from the table files. If an old backup may have leaked, have users revoke the dashboard's OAuth grant (GitHub → Settings → Applications → Authorized OAuth Apps) and sign in again, or revoke all grants for the OAuth App. Rolling back to a pre-#184 build after the upgrade breaks tasks and GitHub calls until each user signs in again, because the plaintext column is now empty.

**`/var/secrets/preview.env`:**

```bash
GITHUB_TOKEN=<personal-access-token>
GITHUB_WEBHOOK_SECRET=<webhook-secret>
PREVIEW_DOMAIN=yourdomain.com
WEBHOOK_PORT=3100
ALLOWED_REPOS=yourorg/repo1,yourorg/repo2
```

These keys are never forwarded into previews, even if a repo lists them in `hostSecrets`.

> **Upgrading from a version before #184:** older versions copied the GitHub token into every preview container (`/etc/preview-token` and the app's git remote), where app code could read it. `deploy.sh` runs `preview scrub-tokens` to remove those copies, but that doesn't undo the exposure, so rotate `GITHUB_TOKEN` here if you use one.

**`/var/secrets/claude/oauth_token`:**

```
sk-ant-oat01-XXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX
```

Just the raw token string, no newlines or quotes.

### Isolating preview builds

A repo's `preview-config.nix` is untrusted code, and it runs on the host: `preview` evaluates and builds it to make the container. Without isolation it could read `/run/dashboard-secrets`, `/var/secrets` or any other host file, and copy it into the world-readable `/nix/store` or out over the network. Two layers prevent that:

- **Evaluation** runs through `systemd-run` as a throwaway `DynamicUser`, with Nix's `restrict-eval` on, no network (`IPAddressDeny=any`), and `/run/dashboard-secrets`, `/var/secrets`, `/var/lib` and `/etc/nixos` hidden. Restricted evaluation alone isn't enough, because `builtins.getFlake "path:/some/dir"` still copies any directory the evaluator can read into the store. The config only sees nixpkgs, which is pinned by `/etc/nixos/flake.lock`, and its own file.
- **Builds** run in nix-daemon's sandbox as `nixbld` users. Fixed-output builds share the host network, so the `tekton-nixbld` firewall chain in `configuration.nix` rejects their traffic to loopback (`/internal/secrets`, the webhook's `/internal/token` and Caddy's admin API trust loopback), to private and link-local addresses (containers, cloud metadata), and to anything but DNS there.

`preview` refuses to build while the chain is missing.

What this means for repo configs: a `preview-config.nix` can't read files outside the store, can't call `builtins.getEnv`, and can't fetch at evaluation time (`fetchTarball`, `fetchGit`, `fetchurl`). Fetchers inside derivations (`pkgs.fetchurl`, `fetchFromGitHub`, …) still work.

**Upgrading an existing server:** `deploy.sh` never copies `configuration.nix`, so copy the `networking.firewall.extraCommands` and `extraStopCommands` blocks, and the swap settings (`swapDevices`, `zramSwap`), into `/etc/nixos/configuration.nix` on the server. Then run `./deploy.sh <server> nix`. It warns if the chain is still missing. To check by hand:

```bash
iptables -w -S tekton-nixbld && iptables -w -C OUTPUT -m owner --gid-owner nixbld -j tekton-nixbld && echo OK
```

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
| Dashboard down after a reboot; "Failed to load environment files" | `/run/dashboard-secrets/secrets.env` is on tmpfs and a reboot wipes it. Run `server-config/dashboard-secrets.sh set <server>` and paste the three values from 1Password; see [Dashboard secrets](#dashboard-secrets). |
| "SECRETS_ENCRYPTION_KEY is not set" (or `JWT_SECRET`, `GITHUB_CLIENT_SECRET`) | The unit doesn't load the `/run` file, or the file lacks that variable. Check the `EnvironmentFile` list in `configuration.nix`, then run `set` again. |
| "SECRETS_ENCRYPTION_KEY does not decrypt any of N stored encrypted value(s)" | The key isn't the one the data was encrypted with: wrong 1Password item, or a pasting mistake. Fix the item and run `set` again. Don't generate a new key: that loses every stored secret. See [Losing or changing the key](#losing-or-changing-the-key). |
| `deploy.sh`: "Dashboard secrets are not set up correctly" | It lists what's wrong. Missing or incomplete `/run` file: run `set`. `dashboard.env` still defines a secret, or an old key file still exists: save the value in 1Password, run `verify-key` for the key, then remove it. `configuration.nix` or the loaded unit doesn't read the `/run` file: see step 7 of [Upgrading an existing server](#upgrading-an-existing-server). |
| `set`: "Contains a control character (or invalid UTF-8)" | The paste picked up a newline or other control character. Copy the field again and paste it alone. For `JWT_SECRET` or `GITHUB_CLIENT_SECRET` you can also generate a new value. Never for `SECRETS_ENCRYPTION_KEY`: it can't be changed. Run `check-live` to see whether the live key itself has such characters. |
| `set`: "Doesn't match the key the running dashboard uses" | The pasted key isn't the live one. Copy it again with `copy-live` and check it with `verify-key`. Use `--new-key` only for a real key change, which loses every stored secret. |
| `deploy.sh`: "Leftover temp file … from an interrupted set" | A `set` was killed before it could clean up, and the file holds secrets. Delete it on the server (`rm /run/dashboard-secrets/.secrets.env.*`). |
| `set` / `verify-key`: "No terminal" or "run this from an interactive terminal" | Run it directly in a terminal, not through a pipe or a non-interactive wrapper: the prompts need a tty on the server. |
| `deploy.sh`: "Dashboard failed to start after deploy" | It prints the last log lines. Usually one of the rows above. |
| `server-setup.sh`: "already has a dashboard install, but /run/dashboard-secrets/secrets.env is missing" | Run `set` with the values from 1Password and rerun setup. Don't override with `ALLOW_NEW_DASHBOARD_SECRETS=1` unless the old values are lost. |
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

The generated `.env` includes a random `SECRETS_ENCRYPTION_KEY`; the backend won't start without one. An `.env` from an earlier build that only has `SECRETS_ENCRYPTION_KEY_FILE=.secrets-key` needs a `SECRETS_ENCRYPTION_KEY=` line instead. To keep what you already encrypted locally, set it to the contents of `dashboard/backend/.secrets-key` (then delete that file); otherwise generate one with `openssl rand -base64 32`.

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
