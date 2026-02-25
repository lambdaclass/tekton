# Adding a new repo to tekton previews

Tekton knows nothing about the repos it deploys. Each repo ships a
`preview-config.nix` file at its root that fully describes how to build and
run the app inside an isolated NixOS container. Tekton fetches that file from
GitHub when a PR is opened, builds a container closure from it (cached by
commit SHA), and starts the container.

To add a new repo you need to do two things:

1. Add `preview-config.nix` to the root of the target repo.
2. Register the repo in the webhook allowlist.

---

## Step 1 — Write `preview-config.nix`

Start from the annotated example at
[`example_service/preview-config.nix`](../example_service/preview-config.nix).
It is a complete, working configuration for a simple Python/Flask app and
covers every pattern described below. Copy it and adapt for your stack.

For more complex setups (e.g. Elixir/Phoenix + React SPA monorepos with
multiple frontends, container-local databases, and extra subdomains), use
the minimal skeleton below as a starting point and expand from there.

### Minimal skeleton

```nix
{ config, lib, pkgs, ... }:

let
  meta = {
    setupService = "setup-myapp";        # oneshot service that clones + builds
    appServices  = [ "myapp" ];          # long-running services started after setup
    database     = "container";          # "container" | "host"
    routes       = [
      { path = "/api/*"; port = 4000; }
      { path = "/";      port = 3000; }
    ];
    hostSecrets  = [];                   # keys forwarded from the host secrets file
    extraHosts   = [];                   # additional subdomains
  };
in
{
  boot.isContainer = true;

  networking.useDHCP = false;
  networking.useHostResolvConf = false;
  services.resolved.enable = true;
  networking.nameservers = [ "8.8.8.8" "1.1.1.1" ];

  networking.firewall.allowedTCPPorts = [ 3000 4000 ];

  # ... your systemd services here ...

  users.users.preview = {
    isNormalUser = true;
    home  = "/home/preview";
    shell = pkgs.bash;
  };

  # Required — exposes meta to tekton before boot and inside the container
  system.build.previewMeta = pkgs.writeText "preview-meta.json" (builtins.toJSON meta);
  environment.etc."preview-meta.json".text = builtins.toJSON meta;

  system.stateVersion = "24.11";
}
```

---

## The `meta` block

All six fields are required. Tekton aborts with a clear error if any is missing.

### `setupService`

Name of the oneshot systemd service that clones the repo, installs
dependencies, runs build steps, and runs database migrations. Must match the
service you define in the same file.

### `appServices`

List of long-running services started after setup completes (backend server,
SPA file server, etc.).

### `database`

| Value | Behaviour |
|---|---|
| `"container"` | Your app manages its own database inside the container (or has none). Configure `services.postgresql` in the nix file; tekton does not touch the database. |
| `"host"` | Tekton creates a dedicated PostgreSQL database on the host, generates a random password, and injects `DATABASE_URL` into `/etc/preview.env`. |

### `routes`

Array of `{ path, port }` objects. Caddy matches the most-specific path first;
the root catch-all `"/"` must always be last.

```nix
routes = [
  { path = "/api/*"; port = 4000; }
  { path = "/";      port = 3000; }
];
```

Add `stripPrefix = true` to strip the matched prefix before forwarding (useful
for admin panels at a sub-path):

```nix
{ path = "/admin/*"; port = 3000; stripPrefix = true; }
```

### `hostSecrets`

List of environment variable names forwarded from the host's
`/var/secrets/preview.env` into the container's `/etc/preview.env`. Use this
for third-party API keys that must not be generated per-preview.

```nix
hostSecrets = [ "STRIPE_SECRET_KEY" "POSTMARK_API_KEY" ];
```

Tekton warns (but does not abort) if a listed key is missing on the host.

### `extraHosts`

Additional Caddy virtual-host blocks at `<prefix>-<slug>.<domain>`. Useful
when the app has separate subdomains (e.g. a landing page).

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

## Environment available inside the container

Tekton writes two files into the container filesystem before it boots.

### `/etc/preview-token` (mode 604)

Contains a short-lived GitHub App installation token. Use it to authenticate
`git clone` / `git fetch`:

```bash
PREVIEW_TOKEN=$(cat /etc/preview-token 2>/dev/null || echo "")
AUTHED_URL=$(echo "$PREVIEW_REPO_URL" | sed "s|https://|https://x-access-token:$PREVIEW_TOKEN@|")
git clone --depth 1 --branch "$PREVIEW_BRANCH" "$AUTHED_URL" /home/preview/app
```

### `/etc/preview.env` (mode 644)

Always contains:

| Variable | Example |
|---|---|
| `PREVIEW_REPO_URL` | `https://github.com/myorg/myapp.git` |
| `PREVIEW_BRANCH` | `feature/my-pr` |
| `PREVIEW_HOST` | `myapp-42.preview.example.com` |
| `PREVIEW_URL` | `https://myapp-42.preview.example.com` |
| `DATABASE_URL` | *(only when `database = "host"`)* |

Plus any keys listed in `hostSecrets`.

Source it at the start of your setup script:

```bash
set -a; source /etc/preview.env; set +a
```

---

## Setup service pattern

The setup service is a `RemainAfterExit = true` oneshot that clones the repo,
builds the app, and runs migrations. App services depend on it so systemd
waits for it to succeed before starting them.

```nix
systemd.services.setup-myapp = {
  description = "Setup myapp preview (clone, build, migrate)";
  after  = [ "systemd-resolved.service" ];
  wants  = [ "systemd-resolved.service" ];
  before = [ "myapp.service" ];
  path   = [ pkgs.bash pkgs.coreutils pkgs.git pkgs.openssl ];
  serviceConfig = {
    Type             = "oneshot";
    RemainAfterExit  = true;
    User             = "preview";
    WorkingDirectory = "/home/preview";
    TimeoutStartSec  = "900";   # 15 min for slow compilers
  };
  script = ''
    set -euo pipefail
    set -a; source /etc/preview.env; set +a

    # ... generate secrets, clone, build, migrate ...
  '';
};
```

### Skip rebuild on container restart

If the container is restarted (host reboot, etc.) the code is already built.
Check for a stack-specific built artifact and exit early:

```bash
APP_DIR="/home/preview/app"
# Adapt the artifact check to your stack:
#   Node.js:  [ -f "$APP_DIR/dist/index.js" ]
#   Elixir:   [ -f "$APP_DIR/_build/prod/rel/myapp/bin/myapp" ]
#   Python:   [ -d "$APP_DIR/venv" ]
if [ -d "$APP_DIR/.git" ] && [ -f "$APP_DIR/dist/index.js" ] && [ ! -f /tmp/force-rebuild ]; then
  echo "Already built, skipping setup."
  exit 0
fi
rm -f /tmp/force-rebuild
```

`preview update <slug>` touches `/tmp/force-rebuild` before restarting the
setup service, so explicit updates always do a full rebuild.

### Waiting for PostgreSQL (container DB)

When using `database = "container"`, PostgreSQL may not be ready even after
`postgresql.service` starts. Add a readiness loop before running migrations:

```bash
for i in $(seq 1 30); do
  pg_isready -U myapp -d myapp -q && break
  echo "Waiting for PostgreSQL ($i/30)..."
  sleep 2
done
pg_isready -U myapp -d myapp || { echo "PostgreSQL not ready after 60s"; exit 1; }
```

Add `pkgs.postgresql` to the service `path` list so `pg_isready` is on `$PATH`.

### PHX_SERVER during Elixir eval

When running `mix release eval` for migrations or seeds, set `PHX_SERVER=false`
inline to prevent the HTTP endpoint from starting:

```bash
PHX_SERVER=false ./rel/myapp/bin/myapp eval "MyApp.Release.migrate()"
```

---

## Per-preview secrets

Generate stable secrets on first run and persist them so container restarts
don't regenerate them (which would invalidate signed sessions):

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

Pass both files to your app service:

```nix
serviceConfig.EnvironmentFile = [
  "/home/preview/.myapp-secrets.env"
  "/etc/preview.env"
];
```

---

## Nix string escaping

Inside Nix `''...''` strings, `${...}` is Nix interpolation. To emit a
literal bash variable like `${PREVIEW_HOST}`, write `''${PREVIEW_HOST}`:

```nix
script = ''
  echo "PHX_HOST=''${PREVIEW_HOST}" >> "$SECRETS_FILE"
'';
```

This is the most common cause of build failures when writing a new
`preview-config.nix`.

---

## Static file servers

Use `pkgs.static-web-server` (in nixpkgs) to serve SPA builds. It starts
instantly and requires no network access:

```nix
serviceConfig.ExecStart =
  "${pkgs.static-web-server}/bin/static-web-server --port 3000 --root dist --page-fallback dist/index.html";
```

---

## Step 2 — Register the repo in the webhook allowlist

On the server, add the repo to `ALLOWED_REPOS` in `/var/secrets/preview.env`
and restart the webhook service:

```bash
# /var/secrets/preview.env
ALLOWED_REPOS=myorg/existing-repo,myorg/myapp
```

```bash
systemctl restart preview-webhook
```

Then install a GitHub webhook on the new repo:

- **Payload URL:** `https://webhook.<PREVIEW_DOMAIN>/webhook/github`
- **Content type:** `application/json`
- **Secret:** value of `WEBHOOK_SECRET` from the server secrets
- **Events:** Pull requests only

Once active, opening a PR automatically creates a preview and posts the URL
to the PR description.

---

## Testing without a webhook

You can also create and destroy previews manually:

```bash
# Create
preview create myorg/myapp feature-branch --slug myapp-test

# Watch the build
preview logs myapp-test --follow

# Destroy
preview destroy myapp-test
```
