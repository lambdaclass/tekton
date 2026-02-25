# How to add a new service to tekton previews

Tekton knows nothing about the repos it deploys. Instead, each repo ships a
`preview-config.nix` at its root that fully describes how to build and run
the app inside an isolated NixOS container. Tekton fetches that file from
GitHub at PR-open time, builds a container closure from it (cached by commit
SHA), and starts the container.

To add a new repo you need to do two things:

1. Add `preview-config.nix` to the root of the target repo.
2. Add the repo to the webhook allowlist in tekton's config.

---

## 1. Create `preview-config.nix` in your repo

The file must be a valid NixOS module and must expose two extra derivations:

- `system.build.previewMeta` — a JSON file that tekton reads **before**
  starting the container to learn about routing, services, and secrets.
- `environment.etc."preview-meta.json"` — the same JSON, available inside
  the running container at `/etc/preview-meta.json`.

### Minimal skeleton

```nix
{ config, lib, pkgs, ... }:

let
  meta = {
    setupService = "setup-myapp";          # oneshot systemd service that clones + builds
    appServices  = [ "myapp-backend" ];    # services started after setup completes
    database     = "container";            # "container" | "host"
    routes       = [
      { path = "/api/*"; port = 4000; }
      { path = "/";      port = 3000; }
    ];
    hostSecrets  = [];   # keys to forward from the host's /var/secrets/preview.env
    extraHosts   = [];   # additional subdomains (see below)
  };
in
{
  boot.isContainer = true;

  networking.useDHCP = false;
  networking.useHostResolvConf = false;
  services.resolved.enable = true;
  networking.nameservers = [ "8.8.8.8" "1.1.1.1" ];

  networking.firewall.allowedTCPPorts = [ 3000 4000 ];

  # ... your services here ...

  system.build.previewMeta = pkgs.writeText "preview-meta.json" (builtins.toJSON meta);
  environment.etc."preview-meta.json".text = builtins.toJSON meta;

  system.stateVersion = "24.11";
}
```

---

## 2. The `meta` block — required fields

All six fields must be present; tekton will abort with a clear error if any is
missing.

### `setupService`

Name of the oneshot systemd service that clones the repo, installs
dependencies, runs build steps, and runs migrations. Must match the service
you define in the same file.

```nix
setupService = "setup-myapp";
```

### `appServices`

List of services started (in parallel) after setup completes. These are the
long-running processes (backend server, frontend file server, …).

```nix
appServices = [ "myapp-backend" "myapp-frontend" ];
```

### `database`

Whether the app needs a PostgreSQL database.

| Value | Behaviour |
|---|---|
| `"container"` | Postgres runs inside the container (full isolation). You configure `services.postgresql` in the same nix file and manage the DB in your setup script. |
| `"host"` | Tekton creates a dedicated database on the host Postgres, generates a random password, and injects `DATABASE_URL` into `/etc/preview.env`. |

### `routes`

Array of `{ path, port }` objects that Caddy uses to reverse-proxy incoming
requests into the container. Routes are matched most-specific first (longest
path wins). The root catch-all `"/"` must always be last.

```nix
routes = [
  { path = "/api/*"; port = 4000; }
  { path = "/ws";    port = 4000; }
  { path = "/";      port = 3000; }
];
```

Add `stripPrefix = true` to strip the matched prefix before forwarding (useful
for admin panels mounted at a sub-path):

```nix
{ path = "/admin/*"; port = 3000; stripPrefix = true; }
```

Tekton generates a Caddy `handle_path` block for strip-prefix routes so the
app receives requests at `/`, not `/admin/…`.

### `hostSecrets`

List of environment variable names that should be forwarded from the host's
`/var/secrets/preview.env` into the container's `/etc/preview.env`. Use this
for third-party API keys (Stripe, Postmark, etc.) that must not be generated
per-preview.

```nix
hostSecrets = [ "STRIPE_SECRET_KEY" "POSTMARK_API_KEY" ];
```

The keys must already exist in `/var/secrets/preview.env` on the host. Tekton
will warn (but not abort) if a listed key is missing.

### `extraHosts`

Additional Caddy virtual-host blocks generated as `<prefix>-<slug>.<domain>`.
Useful when the app has separate subdomains (e.g. a landing page on a
different domain).

```nix
extraHosts = [
  {
    prefix = "landing";
    routes = [ { path = "/"; port = 3002; } ];
  }
];
```

Set to `[]` if you don't need extra subdomains.

---

## 3. Environment available inside the container

Tekton writes two files into the container before it boots.

### `/etc/preview-token` (mode 600, root-only)

Contains the GitHub App installation token. Your setup script reads this to
authenticate `git clone` / `git fetch` without exposing the token to other
processes:

```bash
PREVIEW_TOKEN=$(cat /etc/preview-token 2>/dev/null || echo "")
AUTHED_URL=$(echo "$PREVIEW_REPO_URL" | sed "s|https://|https://x-access-token:$PREVIEW_TOKEN@|")
git clone --depth 1 --branch "$PREVIEW_BRANCH" "$AUTHED_URL" /home/preview/app
```

### `/etc/preview.env` (mode 644)

Always contains:

| Variable | Example value |
|---|---|
| `PREVIEW_REPO_URL` | `https://github.com/myorg/myapp.git` |
| `PREVIEW_BRANCH` | `feature/my-pr` |
| `PREVIEW_HOST` | `myapp-42.preview.example.com` |
| `PREVIEW_URL` | `https://myapp-42.preview.example.com` |
| `DATABASE_URL` | *(only when `database = "host"`)* |

Plus any keys listed in `hostSecrets`.

Source this file at the start of your setup script to access these values:

```bash
set -a; source /etc/preview.env; set +a
```

### Bash variable escaping in Nix `''...''` strings

Nix `''...''` multi-line strings treat `${...}` as Nix interpolation. To
emit a literal bash variable reference like `${PREVIEW_HOST}` you **must**
write `''${PREVIEW_HOST}`:

```nix
script = ''
  echo "PHX_HOST=''${PREVIEW_HOST}" >> "$SECRETS_FILE"
'';
```

This is the single most common source of build failures when writing a new
`preview-config.nix`.

---

## 4. Setup service pattern

The setup service is a oneshot that clones the repo, builds the app, and runs
migrations. It must be a `RemainAfterExit = true` oneshot so systemd knows
it succeeded and the app services can start.

```nix
systemd.services.setup-myapp = {
  description = "Setup myapp preview (clone, build, migrate)";
  after  = [ "systemd-resolved.service" "postgresql.service" ];
  wants  = [ "systemd-resolved.service" "postgresql.service" ];
  before = [ "myapp-backend.service" "myapp-frontend.service" ];
  path   = [ pkgs.bash pkgs.coreutils pkgs.git pkgs.nodejs_22 pkgs.openssl ];
  serviceConfig = {
    Type             = "oneshot";
    RemainAfterExit  = true;
    User             = "preview";
    WorkingDirectory = "/home/preview";
    TimeoutStartSec  = "900";  # 15 min — compilation can be slow
  };
  script = ''
    set -euo pipefail
    set -a; source /etc/preview.env; set +a

    # ... generate per-preview secrets, clone, build, migrate ...
  '';
};
```

### Fast container restarts

If the container is restarted (e.g. host reboot) the code is already built.
Skip the expensive setup with an early-exit guard:

```bash
APP_DIR="/home/preview/app"
if [ -d "$APP_DIR/.git" ] && [ -f "$APP_DIR/dist/index.js" ] && [ ! -f /tmp/force-rebuild ]; then
  echo "Already built, skipping setup."
  exit 0
fi
rm -f /tmp/force-rebuild
```

`preview update <slug>` touches `/tmp/force-rebuild` inside the container
before restarting the setup service, so explicit update requests always
trigger a full rebuild.

### Database readiness (container DB only)

When using `database = "container"`, PostgreSQL may not be ready to accept
connections the instant the unit starts even though `postgresql.service` is
listed in `wants`. Add a readiness loop before running migrations:

```bash
for i in $(seq 1 30); do
  pg_isready -U myapp -d myapp -q && break
  echo "Waiting for PostgreSQL ($i/30)..."
  sleep 2
done
pg_isready -U myapp -d myapp || { echo "PostgreSQL not ready"; exit 1; }
```

Add `pkgs.postgresql` to the service `path` list so `pg_isready` is
available.

---

## 5. Per-preview secrets generation

Generate stable secrets on first run and persist them so container restarts
don't regenerate them (which would invalidate signed sessions, etc.):

```bash
SECRETS_FILE="/home/preview/.myapp-secrets.env"
if [ ! -f "$SECRETS_FILE" ]; then
  {
    echo "SECRET_KEY_BASE=$(openssl rand -hex 64)"
    echo "DATABASE_URL=postgresql://myapp@localhost/myapp"
  } > "$SECRETS_FILE"
  chmod 600 "$SECRETS_FILE"
fi

# Load secrets, then re-source preview.env so hostSecrets always win
set -a
source "$SECRETS_FILE"
source /etc/preview.env
set +a
```

The re-source order matters: load your generated secrets first, then overlay
`preview.env` so that any key forwarded via `hostSecrets` takes precedence.

Pass the secrets file to your app service via `EnvironmentFile`:

```nix
serviceConfig.EnvironmentFile = [
  "/home/preview/.myapp-secrets.env"
  "/etc/preview.env"
];
```

---

## 6. Static file servers for SPAs

Use `pkgs.static-web-server` (already in nixpkgs) instead of `npx serve` or
similar npm-fetched tools. It requires no network access at container start
time:

```nix
systemd.services.myapp-frontend = {
  # ...
  path = [ pkgs.static-web-server ];
  serviceConfig.ExecStart =
    "${pkgs.static-web-server}/bin/static-web-server --port 3000 --root dist --page-fallback dist/index.html";
};
```

---

## 7. Add the repo to the webhook allowlist

In the tekton repo, add the repo to `ALLOWED_REPOS` in
`/var/secrets/preview-webhook.env` (or `WEBHOOK_ALLOWED_REPOS` — check
`server-config/preview-webhook/src/config.ts` for the exact env var name) on
the server:

```
WEBHOOK_ALLOWED_REPOS=myorg/existing-repo,myorg/myapp
```

Then install a GitHub webhook on the new repo pointing to
`https://webhook.<preview-domain>/webhook/github` with:

- **Content type:** `application/json`
- **Secret:** the value of `WEBHOOK_SECRET` from the server secrets
- **Events:** Pull requests only (`pull_request`)

Once the webhook is active, opening a PR will automatically create a preview
and post the URL as a comment on the PR.

---

## 8. Reference examples

| Repo type | Config file |
|---|---|
| Elixir/Phoenix + multiple React SPAs | `server-config/vertex-preview-config.nix` |
| Elixir/Phoenix API + single React SPA | `server-config/stablecoin-preview-config.nix` |
| Node.js / generic | `server-config/preview-config.nix` |

Copy the closest example as your starting point and adapt the service names,
build commands, and ports to your app.
