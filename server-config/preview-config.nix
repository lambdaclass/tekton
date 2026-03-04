# NixOS configuration template for Node.js preview containers
# Copy this file to your repo root as preview-config.nix and customise.
# This file is fetched by tekton at preview-create time.
# Built by: tekton's preview.sh using nix build --impure --expr
#
# system.build.previewMeta  — read by tekton before container start (routing, services, DB)
# environment.etc."preview-meta.json"  — same JSON available inside the running container
{ config, lib, pkgs, ... }:

let
  meta = {
    setupService = "setup-preview";
    appServices  = [ "preview-app" ];
    database     = "host";
    routes       = [ { path = "/"; port = 3000; } ];
    hostSecrets  = [];
    extraHosts   = [];
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

  # Open port 3000 for the preview app
  networking.firewall.allowedTCPPorts = [ 3000 ];

  # Setup preview: clone repo, install deps, build
  systemd.services.setup-preview = {
    description = "Setup preview deployment (clone, install, build)";
    # Not started at boot — triggered from host after container is up
    after = [ "systemd-resolved.service" ];
    wants = [ "systemd-resolved.service" ];
    before = [ "preview-app.service" ];
    path = [ pkgs.bash pkgs.coreutils pkgs.findutils pkgs.gnugrep pkgs.gnused pkgs.git pkgs.nodejs_22 ];
    serviceConfig = {
      Type = "oneshot";
      RemainAfterExit = true;
      User = "preview";
      WorkingDirectory = "/home/preview";
    };
    script = ''
      set -euo pipefail

      # Load environment
      if [ ! -f /etc/preview.env ]; then
        echo "ERROR: /etc/preview.env not found"
        exit 1
      fi
      set -a
      source /etc/preview.env
      set +a

      APP_DIR="/home/preview/app"

      # On container restart, skip setup if the app is already built.
      # This avoids git fetch with expired tokens.
      # Explicit rebuilds go through 'preview update' which clears the marker.
      if [ -d "$APP_DIR/node_modules" ] && [ ! -f /tmp/force-rebuild ]; then
        echo "App already built, skipping setup (container restart)."
        exit 0
      fi
      rm -f /tmp/force-rebuild

      # Build authenticated URL from the root-only token file
      PREVIEW_TOKEN=$(cat /etc/preview-token 2>/dev/null || echo "")
      AUTHED_URL=$(echo "$PREVIEW_REPO_URL" | sed "s|https://|https://x-access-token:$PREVIEW_TOKEN@|")

      if [ -d "$APP_DIR/.git" ]; then
        # Update: refresh remote URL with current token, then fetch latest
        echo "Updating existing repo..."
        ${pkgs.git}/bin/git -C "$APP_DIR" remote set-url origin "$AUTHED_URL"
        ${pkgs.git}/bin/git -C "$APP_DIR" fetch origin
        ${pkgs.git}/bin/git -C "$APP_DIR" reset --hard "origin/$PREVIEW_BRANCH"
      else
        # Fresh clone
        echo "Cloning $PREVIEW_REPO_URL (branch: $PREVIEW_BRANCH)..."
        ${pkgs.git}/bin/git clone --branch "$PREVIEW_BRANCH" --single-branch "$AUTHED_URL" "$APP_DIR"
      fi
      cd "$APP_DIR"

      # Install dependencies
      if [ -f package-lock.json ]; then
        echo "Installing dependencies (npm ci)..."
        ${pkgs.nodejs_22}/bin/npm ci
      else
        echo "No lockfile found, installing dependencies (npm install)..."
        ${pkgs.nodejs_22}/bin/npm install
      fi

      # Build
      echo "Building application..."
      ${pkgs.nodejs_22}/bin/npm run build

      echo "Setup complete."
    '';
  };

  # Preview app: run the built application
  systemd.services.preview-app = {
    description = "Preview application";
    # Not started at boot — triggered from host after container is up
    after = [ "setup-preview.service" ];
    requires = [ "setup-preview.service" ];
    path = [ pkgs.bash pkgs.coreutils pkgs.nodejs_22 ];
    serviceConfig = {
      Type = "simple";
      User = "preview";
      WorkingDirectory = "/home/preview/app";
      EnvironmentFile = "/etc/preview.env";
      ExecStart = "${pkgs.nodejs_22}/bin/npm start";
      Restart = "on-failure";
      RestartSec = 5;
      MemoryMax = "1G";
      CPUQuota = "100%";
    };
  };

  # Preview user (non-root)
  users.users.preview = {
    isNormalUser = true;
    home = "/home/preview";
    shell = pkgs.bash;
  };

  # SSH access for debugging
  services.openssh = {
    enable = true;
    settings.PermitRootLogin = "yes";
  };

  users.users.root = {};

  system.build.previewMeta = pkgs.writeText "preview-meta.json" (builtins.toJSON meta);
  environment.etc."preview-meta.json".text = builtins.toJSON meta;

  # Packages available in preview containers
  environment.systemPackages = with pkgs; [
    git
    nodejs_22
    curl
    jq
    gh
  ];

  system.stateVersion = "24.11";
}
