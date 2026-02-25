# NixOS configuration for example-service preview containers (Python/Flask)
# Copy this file to the root of your repo as preview-config.nix and adapt it.
# Built by: tekton's preview.sh using nix build --impure --expr
#
# system.build.previewMeta  — read by tekton before container start (routing, services, DB)
# environment.etc."preview-meta.json"  — same JSON available inside the running container
{ config, lib, pkgs, ... }:

let
  meta = {
    # Name of the oneshot service that clones, installs deps, and runs migrations.
    setupService = "setup-example";

    # Long-running services started after setup completes.
    appServices = [ "example-app" ];

    # "container" — app manages its own DB (or has none).
    # "host"      — tekton creates a Postgres DB on the host and injects DATABASE_URL.
    database = "container";

    # Caddy routing: most-specific path wins; "/" must be last.
    # Add stripPrefix = true to strip the matched prefix before forwarding.
    routes = [
      { path = "/"; port = 8000; }
    ];

    # Keys forwarded from /var/secrets/preview.env on the host into /etc/preview.env.
    # Use for third-party API keys that must not be generated per-preview.
    hostSecrets = [];

    # Extra Caddy virtual-host blocks: generates <prefix>-<slug>.<domain>.
    # Useful for apps with separate subdomains (e.g. landing pages).
    extraHosts = [];
  };
in
{
  boot.isContainer = true;

  # Networking — static IP is set by nixos-container, disable DHCP
  networking.useDHCP = false;
  networking.useHostResolvConf = false;
  services.resolved = {
    enable = true;
    settings.Resolve.FallbackDNS = [ "8.8.8.8" "1.1.1.1" ];
  };
  networking.nameservers = [ "8.8.8.8" "1.1.1.1" ];

  # Open the port(s) listed in routes above
  networking.firewall.allowedTCPPorts = [ 8000 ];

  # Setup: clone repo, create venv, install dependencies
  systemd.services.setup-example = {
    description = "Setup example-service preview (clone, install deps)";
    after  = [ "systemd-resolved.service" ];
    wants  = [ "systemd-resolved.service" ];
    before = [ "example-app.service" ];
    path   = [ pkgs.bash pkgs.coreutils pkgs.git pkgs.python3 ];
    serviceConfig = {
      Type            = "oneshot";
      RemainAfterExit = true;
      User            = "preview";
      WorkingDirectory = "/home/preview";
      TimeoutStartSec = "300";
    };
    script = ''
      set -euo pipefail

      # Load environment — provides PREVIEW_HOST, PREVIEW_BRANCH, etc.
      if [ ! -f /etc/preview.env ]; then
        echo "ERROR: /etc/preview.env not found"
        exit 1
      fi
      set -a
      source /etc/preview.env
      set +a

      SECRETS_FILE="/home/preview/.example-secrets.env"

      # Generate stable per-preview secrets on first run (persist across restarts).
      if [ ! -f "$SECRETS_FILE" ]; then
        echo "Generating preview secrets..."
        {
          echo "PORT=8000"
          echo "FLASK_ENV=production"
        } > "$SECRETS_FILE"
        chmod 600 "$SECRETS_FILE"
      fi

      # Load secrets, then re-source preview.env so hostSecrets always win.
      set -a
      source "$SECRETS_FILE"
      source /etc/preview.env
      set +a

      APP_DIR="/home/preview/app"

      # On container restart, skip setup if the app is already installed.
      # 'preview update <slug>' touches /tmp/force-rebuild to force a full rebuild.
      if [ -d "$APP_DIR/.git" ] && [ -d "$APP_DIR/venv" ] && [ ! -f /tmp/force-rebuild ]; then
        echo "App already installed, skipping setup (container restart)."
        exit 0
      fi
      rm -f /tmp/force-rebuild

      # Build an authenticated clone URL from the root-only token file.
      PREVIEW_TOKEN=$(cat /etc/preview-token 2>/dev/null || echo "")
      AUTHED_URL=$(echo "$PREVIEW_REPO_URL" | sed "s|https://|https://x-access-token:$PREVIEW_TOKEN@|")

      if [ -d "$APP_DIR/.git" ]; then
        echo "Updating existing repo..."
        ${pkgs.git}/bin/git -C "$APP_DIR" remote set-url origin "$AUTHED_URL"
        ${pkgs.git}/bin/git -C "$APP_DIR" fetch origin
        ${pkgs.git}/bin/git -C "$APP_DIR" reset --hard "origin/$PREVIEW_BRANCH"
      else
        echo "Cloning $PREVIEW_REPO_URL (branch: $PREVIEW_BRANCH)..."
        ${pkgs.git}/bin/git clone --depth 1 --branch "$PREVIEW_BRANCH" --single-branch "$AUTHED_URL" "$APP_DIR"
      fi

      # Create a virtualenv and install dependencies
      echo "Installing Python dependencies..."
      cd "$APP_DIR/example_service"
      ${pkgs.python3}/bin/python -m venv venv
      venv/bin/pip install --quiet -r requirements.txt

      echo "Example service setup complete."
    '';
  };

  # App service: runs the Flask server
  systemd.services.example-app = {
    description = "Example service Flask app (port 8000)";
    after    = [ "setup-example.service" ];
    requires = [ "setup-example.service" ];
    path     = [ pkgs.bash pkgs.coreutils ];
    serviceConfig = {
      Type        = "simple";
      User        = "preview";
      WorkingDirectory = "/home/preview/app/example_service";
      # Secrets loaded first; /etc/preview.env overlays on top so hostSecrets win.
      EnvironmentFile = [
        "/home/preview/.example-secrets.env"
        "/etc/preview.env"
      ];
      ExecStart   = "/home/preview/app/example_service/venv/bin/python app.py";
      Restart     = "on-failure";
      RestartSec  = 5;
    };
  };

  # Preview user (non-root) — all app processes run as this user
  users.users.preview = {
    isNormalUser = true;
    home         = "/home/preview";
    shell        = pkgs.bash;
  };

  # SSH access for debugging (root password set here for convenience — change in prod)
  services.openssh = {
    enable = true;
    settings.PermitRootLogin = "yes";
  };

  users.users.root = {
    password = "changeme";
  };

  # Required: expose meta to tekton before container boot, and inside the container.
  system.build.previewMeta = pkgs.writeText "preview-meta.json" (builtins.toJSON meta);
  environment.etc."preview-meta.json".text = builtins.toJSON meta;

  environment.systemPackages = with pkgs; [
    git
    python3
    curl
    jq
  ];

  system.stateVersion = "24.11";
}
