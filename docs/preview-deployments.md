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
2. Tekton runs `nix build` to build the NixOS container closure and extract `system.build.previewMeta` (routing, service names, DB mode, host secrets). The config is evaluated in Nix's restricted mode, as a throwaway unprivileged user; see [Untrusted configs](#untrusted-configs)
3. The container is created with `nixos-container create`, given a static IP on `10.100.0.0/24`
4. Tekton writes `/etc/preview.env` into the container filesystem before it boots: preview metadata (`PREVIEW_REPO_URL`, `PREVIEW_BRANCH`, `PREVIEW_HOST`, `PREVIEW_URL`, forwarded host secrets)
5. The container starts and tekton sends the branch's code in (see [Code delivery](#code-delivery)); then the repo's `setupService` clones/builds the app, and `appServices` run the live process(es)
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

Environment variables in `/var/secrets/preview.env`:

| Variable | Required | Description |
|----------|----------|-------------|
| `GITHUB_WEBHOOK_SECRET` | Yes | Secret for verifying webhook signatures |
| `PREVIEW_DOMAIN` | Yes | Domain for preview URLs (e.g., `preview.example.com`) |
| `GITHUB_APP_ID` | Yes | ID of the private GitHub App the webhook authenticates as |
| `GITHUB_APP_INSTALLATION_ID` | Yes | The App's installation ID on your org |
| `GITHUB_APP_PRIVATE_KEY_PATH` | Yes | Path to the App's private key; always `/var/secrets/github-app.pem` (root, `0400`) |
| `WEBHOOK_PORT` | No | Port for webhook server (default: `3100`) |
| `ALLOWED_REPOS` | No | Comma-separated `owner/repo` allowlist (empty = allow all repos owned by the App installation's account) |

`GITHUB_TOKEN` must **not** be set: PAT auth was removed, and the webhook refuses to start if it's there. See [Deployment Guide > GitHub App](deployment-guide.md#2-github-app).

### GitHub tokens

The webhook mints GitHub App installation tokens on demand. Each one expires after an hour and is limited to one repo and one permission:

- PR description edits use `pull_requests: write` on the PR's repo.
- `preview` gets `contents: read` tokens for the repo it's deploying from `GET http://127.0.0.1:3100/internal/token?repo=owner/name[&min_ttl=seconds]`. Only direct localhost requests are answered. The repo must match the `owner/name` format, be in `ALLOWED_REPOS` when that's set, and be owned by the installation's account.

Tokens are cached per repo and permission set and reused while they have at least 5 minutes left. Previews started from the dashboard use the same App token: the dashboard doesn't pass the user's token to `preview`, and `preview` ignores a `GITHUB_TOKEN` in its environment.

### Code delivery

Tekton never gives a preview container a GitHub credential:

1. `preview create`/`update` fetch the branch on the host into a root-only bare mirror, `/var/lib/preview-deploys/.repo-cache/<owner>/<repo>.git`. The App token reaches git as an HTTP header through `GIT_CONFIG_*` environment variables, never in argv, a URL or a file.
2. Once the container is running, `preview` streams a git bundle of the deployed commit over stdin to git running as `preview` in the container, which stores it in `~preview/.tekton-src.git`. Updates send only the new commits, and fall back to the full history when the container doesn't have the previous one.
3. A git `insteadOf` rule in `~preview/.gitconfig` points `PREVIEW_REPO_URL` (and `https://x-access-token:@github.com/<repo>.git`, what configs written for the old `/etc/preview-token` build now that the file is gone) at that bare repo. The setup service clones and fetches `PREVIEW_REPO_URL` as usual and gets the host's commits. The copy has only `PREVIEW_BRANCH` (no other branches or tags), and no Git LFS objects.

Previews created by older tekton versions have a token in `/etc/preview-token` and in the app's git remote. `preview update` removes both before anything else, and `preview scrub-tokens` (run by `./deploy.sh <server> nix`) does it for every preview. Scrubbing removes those two copies but can't undo the exposure: app code in those previews could read the tokens, and copies may remain elsewhere (app files, logs, anything already sent out). App tokens expire within an hour. User OAuth tokens (dashboard-started previews) have the full `repo` scope and don't expire, so **revoke them** once this is deployed: revoke the OAuth app's user grants so everyone signs in again (tracked in #188).

### Untrusted configs

An agent can write the repo's `preview-config.nix`, and the host evaluates and builds it. So a config can't read host files (such as the GitHub App key in `/var/secrets`), environment variables or local services:

- Evaluation runs in Nix's restricted mode, through `systemd-run` as a throwaway `DynamicUser`, with an empty environment and no network. Its root directory holds only `/nix/store` and the nix-daemon socket, so no host file is visible. The config only sees its own file (added to the store) and the nixpkgs pinned by `/etc/nixos/flake.lock`.
- Builds run in nix-daemon's sandbox. A firewall chain keeps fixed-output builds, which share the host network, off loopback and private addresses (IP traffic only).

See [Isolating preview builds](deployment-guide.md#isolating-preview-builds) for the details and the server setup.

**Breaking for repo configs (#204):** a config can't fetch anything at evaluation time. `builtins.fetchTarball`, `builtins.fetchGit` and `builtins.fetchurl` fail, as do `readFile` of host paths and imports of paths relative to the config. `builtins.getEnv` returns an empty string. Derivation fetchers (`pkgs.fetchurl`, `fetchFromGitHub`) still work.

To check a server, this should print nothing:

```bash
TOKEN_RE='gh[opsu]_[A-Za-z0-9]{30,}|x-access-token:[A-Za-z0-9_]{20,}'
grep -rlE "$TOKEN_RE" /var/lib/nixos-containers/*/{home,etc,tmp,root,var/tmp} 2>/dev/null
```

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

### `preview` commands report a stale systemd-machined registration

`preview logs`, `preview update` and `preview create` check that machined's `Leader` for the container is the container's real init before using `nixos-container run`. When it isn't, every `nixos-container run <slug>` fails with `nsenter: reassociate to namespaces failed: No such process`. This happens when the container's previous instance didn't finish shutting down, for example because processes were left running inside it. Clearing the registration takes a host reboot. Until then, reach the container through its real init:

```bash
init=$(head -1 /sys/fs/cgroup/machine.slice/container@<slug>.service/payload/init.scope/cgroup.procs)
nsenter -t $init -m -u -i -n -p -- /run/current-system/sw/bin/systemctl start --no-block <setup-service> <app-services...>
nsenter -t $init -m -u -i -n -p -- /run/current-system/sw/bin/journalctl -u <setup-service> -f --no-pager
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
