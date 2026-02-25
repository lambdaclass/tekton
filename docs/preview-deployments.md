# Preview Deployments

The preview system deploys GitHub PR branches as running web apps in isolated nspawn containers, each with its own subdomain and TLS certificate.

## Overview

Each repo self-describes its deployment via a `preview-config.nix` file at the repo root. Tekton fetches that file at preview-create time, builds a NixOS container closure from it, and runs the container. No repo-specific knowledge lives in tekton itself.

Each preview gets:
- A subdomain: `<slug>.<PREVIEW_DOMAIN>`
- TLS via Cloudflare Origin CA wildcard certificate (proxied through Cloudflare)
- Its own nspawn container with a unique IP on the `10.100.0.0/24` subnet

To add support for a new repo, see [docs/adding-a-new-service.md](adding-a-new-service.md).

## Commands Reference

All commands run on the host as root.

### `preview create <owner/repo> <branch> [options]`

Creates a new preview container, fetches `preview-config.nix` from the repo, builds the NixOS closure, and starts the container.

```bash
# Create a preview for any repo (type is determined by the repo's preview-config.nix)
preview create myorg/myapp feature-branch

# With a custom slug
preview create myorg/myapp feature-branch --slug myapp-pr-42
```

Options:
- `--slug <slug>` — Custom slug for the URL (default: auto-generated from repo name + branch)

### `preview destroy <slug>`

Stops the container, removes the Caddy route, and cleans up the database.

```bash
preview destroy myapp-pr-42
```

### `preview update <slug>`

Pulls latest code from the branch and rebuilds inside the container.

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

### `preview build <owner/repo> <branch>`

Pre-builds the NixOS container closure for a repo without creating a preview. Useful for warming the cache before the first deployment.

```bash
preview build myorg/myapp feature-branch
```

## How It Works

1. `preview create` fetches `preview-config.nix` from the repo at the given branch (via GitHub API, cached by commit SHA)
2. Tekton runs `nix build --impure --expr` to build the NixOS container closure and extract `system.build.previewMeta` (routing, service names, DB mode, host secrets)
3. The container is created with `nixos-container create`, given a static IP on `10.100.0.0/24`
4. Tekton writes two files into the container filesystem before it boots:
   - `/etc/preview-token` — GitHub token for authenticated `git clone`/`git fetch`
   - `/etc/preview.env` — preview metadata (`PREVIEW_REPO_URL`, `PREVIEW_BRANCH`, `PREVIEW_HOST`, `PREVIEW_URL`, forwarded host secrets)
5. The container starts; the repo's `setupService` clones/builds the app, then `appServices` run the live process(es)
6. Caddy is configured with routes from `previewMeta.routes` to proxy the subdomain to the container

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

3. **PR slug format**: The webhook generates a slug from the repo name + PR number (e.g., `myapp-42`).

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
- Checks the repo against the allowlist (if `ALLOWED_REPOS` is set)
- Adds a preview link to the PR description via the GitHub API

### Webhook Configuration

Environment variables in `/var/secrets/preview-webhook.env`:

| Variable | Required | Description |
|----------|----------|-------------|
| `WEBHOOK_SECRET` | Yes | Secret for verifying webhook signatures |
| `PREVIEW_DOMAIN` | Yes | Domain for preview URLs (e.g., `preview.example.com`) |
| `WEBHOOK_PORT` | No | Port for webhook server (default: `3100`) |
| `ALLOWED_REPOS` | No | Comma-separated `owner/repo` allowlist (empty = allow all) |

## Troubleshooting

### Browser shows `NET::ERR_CERT_AUTHORITY_INVALID`

The browser is connecting directly to the server and seeing the Cloudflare Origin CA certificate, which is not trusted by browsers. This means traffic is not going through Cloudflare's proxy. Check:

1. **SSL/TLS mode**: In Cloudflare Dashboard > SSL/TLS > Overview, ensure it's set to **Full (strict)**, not "Full" or "Flexible"
2. **DNS proxy status**: In Cloudflare DNS, ensure both `*.preview.example.com` and `preview.example.com` records are set to **Proxied** (orange cloud), not "DNS only" (grey cloud)
3. **DNS propagation**: If you just added the domain to Cloudflare, nameserver changes can take up to 24 hours to propagate. Check with `dig +short your-slug.preview.example.com` — if it returns your server IP directly (instead of a Cloudflare IP), DNS hasn't propagated yet
4. **Zone not active**: Cloudflare will email you when your domain is active. Until then, DNS queries may bypass Cloudflare entirely

### Preview is building but site returns 502

The app is still building. Check build progress:

```bash
preview logs <slug> --follow
```

Elixir/Phoenix builds can take 10-15 minutes on first run.

### Container won't start

Check container status and journal:

```bash
nixos-container status <slug>
machinectl status <slug>
journalctl -u container@<slug>
```

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
# Re-trigger the setup service inside the container (service name comes from preview-config.nix)
nixos-container run <slug> -- systemctl restart <setupService>
```

### Cleaning up a stuck preview

If `preview destroy` fails:

```bash
nixos-container stop <slug> 2>/dev/null
nixos-container destroy <slug>
rm -f /var/lib/preview-deploys/<slug> /var/lib/preview-deploys/<slug>.meta /var/lib/preview-deploys/<slug>.sha
rm -f /etc/caddy/previews/<slug>.caddy
systemctl reload caddy
# If the preview used a host database:
sudo -u postgres psql -c "DROP DATABASE IF EXISTS preview_<slug_underscored>;"
sudo -u postgres psql -c "DROP USER IF EXISTS preview_<slug_underscored>;"
```
