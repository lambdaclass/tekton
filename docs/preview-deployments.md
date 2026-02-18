# Preview Deployments

The preview system deploys GitHub PR branches as running web apps in isolated nspawn containers, each with its own subdomain and TLS certificate.

## Overview

There are two preview types:

| Type | Stack | Ports | Database |
|------|-------|-------|----------|
| **node** | Node.js (`npm ci`, `npm build`, `npm start`) | 3000 | Host PostgreSQL (shared, per-container DB) |
| **vertex** | Elixir/Phoenix backend + React SPA frontends | 4000, 3000, 3001 | Container-local PostgreSQL + Redis |

Each preview gets:
- A subdomain: `<slug>.preview.example.com`
- Auto-provisioned TLS via Caddy + Let's Encrypt
- Its own nspawn container with a unique IP on the `10.100.0.0/24` subnet

## Commands Reference

All commands run on the host as root.

### `preview create <owner/repo> <branch> [options]`

Creates a new preview container, clones the repo, builds, and starts the app.

```bash
# Node.js app
preview create myorg/myapp feature-branch

# With a custom slug
preview create myorg/myapp feature-branch --slug myapp-pr-42

# Vertex (Elixir/Phoenix) app
preview create lambdaclass/vertex feature-branch --type vertex --slug vtx-pr-42
```

Options:
- `--slug <slug>` — Custom slug for the URL (default: auto-generated from repo name + branch)
- `--type <node|vertex>` — Preview type (default: `node`)

### `preview destroy <slug>`

Stops the container, removes the Caddy route, and cleans up the database.

```bash
preview destroy myapp-pr-42
```

### `preview update <slug>`

Pulls latest code from the branch and rebuilds.

```bash
preview update myapp-pr-42
```

### `preview list`

Lists all preview deployments with their status and URL.

```bash
preview list
```

### `preview logs <slug> [--follow]`

Views build and application logs.

```bash
preview logs myapp-pr-42
preview logs myapp-pr-42 --follow
```

### `preview build [--type <node|vertex>]`

Pre-builds the container system closure. This is done automatically on first `preview create`, but you can run it ahead of time to speed up the first deployment.

```bash
preview build                 # node closure
preview build --type vertex   # vertex closure
```

## GitHub Webhook Integration

The webhook server automatically creates, updates, and destroys previews in response to GitHub PR events.

### Setup

1. **Configure the webhook in GitHub**:
   - Go to your repo's **Settings > Webhooks > Add webhook**
   - **Payload URL**: `https://webhook.PREVIEW_DOMAIN/webhook/github`
   - **Content type**: `application/json`
   - **Secret**: The webhook secret from setup (stored in `/var/secrets/preview.env`)
   - **Events**: Select **Pull requests** only

2. **Webhook behavior**:
   | PR Event | Action |
   |----------|--------|
   | `opened` / `reopened` | Creates a new preview, adds preview URL to PR description |
   | `synchronize` (new push) | Pulls latest code and rebuilds |
   | `closed` | Destroys the preview |

3. **PR slug format**: The webhook uses the PR number as the slug (e.g., PR #42 becomes slug `42`, URL `https://42.preview.example.com`).

### Webhook Architecture

The webhook is a Fastify (Node.js) server running on port 3100, proxied through Caddy for TLS at `webhook.PREVIEW_DOMAIN`.

```
GitHub --HTTPS--> Caddy (webhook.preview.example.com)
                    --> localhost:3100 (preview-webhook)
                        --> /run/current-system/sw/bin/preview create/update/destroy
```

The webhook:
- Verifies the GitHub HMAC-SHA256 signature
- Returns 202 immediately, processes in the background
- Checks the repo against the allowlist (if configured)
- Determines preview type based on `VERTEX_REPOS` env var
- Adds a preview link to the PR description via the GitHub API

### Webhook Configuration

Environment variables in `/var/secrets/preview.env`:

| Variable | Required | Description |
|----------|----------|-------------|
| `GITHUB_TOKEN` | Yes | GitHub token for cloning repos and updating PR descriptions |
| `GITHUB_WEBHOOK_SECRET` | Yes | Secret for verifying webhook signatures |
| `PREVIEW_DOMAIN` | Yes | Domain for preview URLs (e.g., `preview.example.com`) |
| `WEBHOOK_PORT` | No | Port for webhook server (default: `3100`) |
| `ALLOWED_REPOS` | No | Comma-separated `owner/repo` allowlist (empty = allow all) |
| `VERTEX_REPOS` | No | Comma-separated repos that should use vertex preview type |

## How Node Previews Work

1. `preview create` allocates an IP, creates a PostgreSQL database on the host, and spins up a container
2. `/etc/preview.env` is written into the container with `DATABASE_URL`, `PREVIEW_REPO_URL`, etc.
3. The `setup-preview` systemd service clones the repo, runs `npm ci`, and `npm run build`
4. The `preview-app` service runs `npm start` on port 3000
5. Caddy routes `<slug>.PREVIEW_DOMAIN` to the container's port 3000

## How Vertex Previews Work

Vertex previews are more complex due to the Elixir/Phoenix + React SPA monorepo:

1. `preview create --type vertex` spins up a container with **its own PostgreSQL and Redis** (no shared host DB)
2. `/etc/preview.env` includes generated secrets (`SECRET_KEY_BASE`, `JWT_SECRET`, `DATABASE_ENCRYPTION_KEY`)
3. The `setup-vertex` service:
   - Clones the repo
   - Builds the Elixir backend (`mix deps.get`, `mix compile`, `mix release`)
   - Builds two React frontends (admin on port 3000, foods on port 3001)
   - Runs database migrations and seeds
4. Three services start: `vertex-backend` (port 4000), `vertex-frontend-admin` (port 3000), `vertex-frontend-foods` (port 3001)
5. Caddy routes traffic based on path:
   - `/api/*` -> container:4000 (Phoenix backend)
   - `/admin/*` -> container:3000 (admin SPA)
   - Everything else -> container:3001 (foods SPA)

### Vertex Container Stack

Each vertex container includes:
- Erlang 27, Elixir 1.18, Node.js 22, pnpm
- PostgreSQL (local, trust auth)
- Redis
- Chromium (for ChromicPDF)

### Vertex Environment Variables

These are auto-generated per container:

| Variable | Value |
|----------|-------|
| `DATABASE_URL` | `postgresql://vertex@localhost:5432/vertex` |
| `SECRET_KEY_BASE` | Random 64-char hex |
| `JWT_SECRET` | Random 64-char hex |
| `DATABASE_ENCRYPTION_KEY` | Random 32-byte base64 |
| `PHX_HOST` | `<slug>.PREVIEW_DOMAIN` |
| `REDIS_URL` | `redis://localhost:6379` |
| `DEPLOY_ENV` | `testing` |

Optional overrides from host secrets: `VERTEX_POSTMARK_API_KEY`, `VERTEX_GOOGLE_CLIENT_ID`, `VERTEX_GOOGLE_CLIENT_SECRET`.

## Troubleshooting

### Preview is building but site returns 502

The app is still building. Check build progress:

```bash
preview logs <slug> --follow
```

Vertex builds can take 10-15 minutes (Elixir compilation is slow).

### Container won't start

Check container status and journal:

```bash
nixos-container status <slug>
machinectl status <slug>
journalctl -u container@<slug>
```

### Database connection issues (node type)

Node previews connect to the host PostgreSQL. Verify:

```bash
# Check PostgreSQL is running
systemctl status postgresql

# Check the container can reach the host
nixos-container run <slug> -- ping -c1 <host-ip>

# Check the database exists
sudo -u postgres psql -l | grep preview_
```

Make sure `networking.firewall.trustedInterfaces = [ "ve-+" ]` is set in `configuration.nix` — without this, container-to-host traffic is blocked.

### Webhook not receiving events

```bash
# Check webhook service
systemctl status preview-webhook
journalctl -u preview-webhook -f

# Test the health endpoint
curl https://webhook.PREVIEW_DOMAIN/health
```

### Manually re-running a build

```bash
# Re-trigger the setup service inside the container
nixos-container run <slug> -- systemctl restart setup-preview  # node
nixos-container run <slug> -- systemctl restart setup-vertex   # vertex
```

### Cleaning up a stuck preview

If `preview destroy` fails:

```bash
nixos-container stop <slug> 2>/dev/null
nixos-container destroy <slug>
rm -f /var/lib/preview-deploys/<slug> /var/lib/preview-deploys/<slug>.type
rm -f /etc/caddy/previews/<slug>.caddy
systemctl reload caddy
# For node type only:
sudo -u postgres psql -c "DROP DATABASE IF EXISTS preview_<slug_underscored>;"
sudo -u postgres psql -c "DROP USER IF EXISTS preview_<slug_underscored>;"
```
