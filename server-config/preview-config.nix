# NixOS configuration for imperative nspawn preview containers
# Built via: nix build /etc/nixos#nixosConfigurations.preview.config.system.build.toplevel
# Used by: nixos-container create <name> --system-path <closure>
#
# Environment flow:
# 1. `preview create` writes /etc/preview.env into the container filesystem
# 2. setup-preview reads it to clone repo, install deps, build
# 3. preview-app reads it to run the app with correct DATABASE_URL, PORT, etc.
{ config, lib, pkgs, ... }:
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
    wantedBy = [ "multi-user.target" ];
    before = [ "preview-app.service" ];
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

      if [ -d "$APP_DIR/.git" ]; then
        # Update: fetch and reset to latest
        echo "Updating existing repo..."
        cd "$APP_DIR"
        ${pkgs.git}/bin/git fetch origin
        ${pkgs.git}/bin/git reset --hard "origin/$PREVIEW_BRANCH"
      else
        # Fresh clone
        echo "Cloning $PREVIEW_REPO_URL (branch: $PREVIEW_BRANCH)..."
        ${pkgs.git}/bin/git clone --branch "$PREVIEW_BRANCH" --single-branch "$PREVIEW_REPO_URL" "$APP_DIR"
        cd "$APP_DIR"
      fi

      # Install dependencies
      echo "Installing dependencies..."
      ${pkgs.nodejs_22}/bin/npm ci

      # Build
      echo "Building application..."
      ${pkgs.nodejs_22}/bin/npm run build

      echo "Setup complete."
    '';
  };

  # Preview app: run the built application
  systemd.services.preview-app = {
    description = "Preview application";
    wantedBy = [ "multi-user.target" ];
    after = [ "setup-preview.service" ];
    requires = [ "setup-preview.service" ];
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

  users.users.root = {
    password = "changeme";
    openssh.authorizedKeys.keys = [
      "ssh-ed25519 AAAA... your-key-here"
    ];
  };

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
