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

Fetches the latest code of the branch on the host, syncs it into the container, and rebuilds. It also scrubs any GitHub token an older tekton version left in the preview (see `scrub-tokens`), even when there are no new commits.

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

### `preview scrub-tokens`

Removes GitHub tokens that tekton versions before #184 left inside previews: deletes `/etc/preview-token` and resets the app's `origin` remote to the plain `https://github.com/<owner>/<repo>.git` URL. It works from the host, so it also covers stopped previews, and it is idempotent. `deploy.sh` runs it after every NixOS config deploy.

Scrubbing removes the tokens but does not undo their exposure: app code in those previews could read them. After deploying this change, rotate the server-wide `GITHUB_TOKEN` in `/var/secrets/preview.env` if you use one, and consider asking users whose OAuth tokens created or updated previews to revoke and re-authorize the dashboard's OAuth app.

```bash
preview scrub-tokens
```

## How It Works

1. `preview create` fetches `preview-config.nix` from the repo at the given branch (via GitHub API, cached by commit SHA)
2. Tekton runs `nix build` (restricted evaluation, as a throwaway user; see [Isolating preview builds](deployment-guide.md#isolating-preview-builds)) to build the NixOS container closure and extract `system.build.previewMeta` (routing, service names, DB mode, host secrets)
3. The container is created with `nixos-container create`, given a static IP on `10.100.0.0/24`
4. Tekton writes `/etc/preview.env` into the container filesystem before it boots: preview metadata (`PREVIEW_REPO_URL`, `PREVIEW_BRANCH`, `PREVIEW_HOST`, `PREVIEW_URL`, `PREVIEW_CODE_PROVIDED=1`, forwarded host secrets)
5. The container starts, and the host provides the code (`sync_code`):
   - the host fetches the branch into a root-only bare cache at `/var/lib/preview-deploys/.repo-cache/<owner>__<repo>.git`, sending the GitHub token as an HTTP header through `GIT_CONFIG_*` environment variables (never in argv, a URL, or a file);
   - it bundles the branch (`git bundle`) and streams the bundle over stdin to git running as the `preview` user inside the container, which clones it into `/home/preview/app` (or, on an update, fetches it and runs `git reset --hard origin/<branch>`). `origin` is the plain `https://github.com/<owner>/<repo>.git` URL. The bundle is deleted afterwards.

   No GitHub token ever enters a preview container.
6. The repo's `setupService` builds the app from `/home/preview/app`, then `appServices` run the live process(es)
7. Caddy is configured with routes from `previewMeta.routes` to proxy the subdomain to the container

The token used for the host fetch is the one `preview` resolves on the host: `GITHUB_TOKEN` from its environment (the dashboard passes the signed-in user's token) or, otherwise, the webhook's `/internal/token` (a GitHub App installation token or the server-wide token). `preview` removes `GITHUB_TOKEN` from its environment once resolved, so it is not passed to the container or to the environment of the `nix build` that evaluates the repo's `preview-config.nix`. `hostSecrets` entries must be plain variable names and are looked up by exact match. Tekton's own keys (`GITHUB_*`, `WEBHOOK_*`, `PREVIEW_*`, `ADMIN_SSH_KEY`, `ALLOWED_REPOS`) are never forwarded.

> **Untrusted configs:** the repo's `preview-config.nix` is evaluated in Nix's restricted mode, as a throwaway unprivileged user with no network, and its builds can't reach loopback or private addresses. So a config can't read host files, environment variables or local services. It also can't fetch anything at evaluation time: use derivation fetchers (`pkgs.fetchurl`, `fetchFromGitHub`) instead of `builtins.fetchTarball` or `fetchGit`. See [Isolating preview builds](deployment-guide.md#isolating-preview-builds).

> **Breaking change (#184):** the container can no longer fetch private repos itself. A repo's `preview-config.nix` must use the code already at `/home/preview/app` instead of cloning it (see [adding-a-new-service.md](adding-a-new-service.md#getting-the-code)). Configs that still clone keep working only for public repos.

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
# Re-trigger the setup service inside the container (service name comes from preview-config.nix).
# This rebuilds the code already in the container; use `preview update <slug>` to pull new commits.
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
