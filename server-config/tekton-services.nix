# Tekton platform services — importable NixOS module
#
# This module provides all tekton services (dashboard, preview webhook,
# containers, Caddy, PostgreSQL) without any hardware-specific config.
# It can be imported into any NixOS server's existing configuration.
#
# Placeholders to substitute before use:
#   - YOUR_DOMAIN         → your preview domain (e.g., example.com)
#   - ssh-ed25519 AAAA... your-key-here → your SSH public key
#   - EXTERNAL_INTERFACE  → main network interface (e.g., enp3s0, eth0)

{ config, lib, pkgs, ... }:
{
  nixpkgs.config.allowUnfree = true;

  # Enable IP forwarding for container NAT
  boot.kernel.sysctl."net.ipv4.ip_forward" = 1;

  # Required for imperative nixos-container create/start/stop
  boot.enableContainers = true;

  # Enable flakes (needed by `agent build` to evaluate the agent flake output)
  nix.settings.experimental-features = [ "nix-command" "flakes" ];

  # NAT for agent/preview containers (ve-+ matches all container veth interfaces)
  networking.nat = {
    enable = true;
    internalInterfaces = [ "ve-+" ];
    externalInterface = "EXTERNAL_INTERFACE";
  };

  # Firewall: allow HTTP (redirects) and HTTPS
  networking.firewall.allowedTCPPorts = [ 80 443 ];

  # Trust container veth interfaces and Tailscale
  networking.firewall.trustedInterfaces = [ "ve-+" "tailscale0" ];

  # Tailscale VPN
  services.tailscale.enable = true;

  # SSH
  services.openssh = {
    enable = true;
    settings = {
      PermitRootLogin = "prohibit-password";
      PasswordAuthentication = false;
    };
  };

  # PostgreSQL for preview deployments and dashboard
  services.postgresql = {
    enable = true;
    enableTCPIP = true;
    settings = {
      listen_addresses = "*";
    };
    authentication = ''
      local dashboard dashboard peer map=dashboard
      local all all peer
      host all all 10.100.0.0/16 md5
    '';
    identMap = ''
      dashboard root dashboard
    '';
    ensureDatabases = [ "dashboard" ];
    ensureUsers = [{
      name = "dashboard";
      ensureDBOwnership = true;
    }];
  };

  # Caddy reverse proxy for preview deployments (TLS via Cloudflare Origin CA)
  services.caddy = {
    enable = true;
    extraConfig = ''
      (cloudflare_tls) {
        tls /var/secrets/cloudflare-origin.pem /var/secrets/cloudflare-origin-key.pem
      }

      dashboard.YOUR_DOMAIN {
        import cloudflare_tls
        reverse_proxy localhost:3200
      }

      import /etc/caddy/previews/*.caddy
    '';
  };

  environment.systemPackages = with pkgs; [
    vim
    git
    curl
    htop
    tmux
    claude-code  # For initial credential setup on host
    nodejs_22   # For building/running the preview webhook
    postgresql  # For preview DB management (psql)
    chromium   # For headless screenshot capture
    (pkgs.writeShellApplication {
      name = "agent";
      runtimeInputs = [ nixos-container openssh ];
      text = builtins.readFile ./agent.sh;
    })
    (pkgs.writeShellApplication {
      name = "preview";
      runtimeInputs = [ coreutils gnused nixos-container openssh curl jq postgresql sudo ];
      text = builtins.readFile ./preview.sh;
    })
  ];

  # Dashboard service
  systemd.services.dashboard = {
    description = "Preview Dashboard";
    wantedBy = [ "multi-user.target" ];
    after = [ "network.target" "caddy.service" "postgresql.service" ];
    requires = [ "postgresql.service" ];
    serviceConfig = {
      Type = "simple";
      Environment = "PATH=/run/current-system/sw/bin:/nix/var/nix/profiles/default/bin";
      ExecStartPre = "${pkgs.coreutils}/bin/mkdir -p /var/lib/dashboard";
      ExecStart = "/opt/dashboard/dashboard";
      Restart = "always";
      RestartSec = 5;
      EnvironmentFile = "/var/secrets/dashboard.env";
      WorkingDirectory = "/opt/dashboard";
    };
  };

  # GitHub PR Preview Webhook service
  systemd.services.preview-webhook = {
    description = "GitHub PR Preview Webhook";
    wantedBy = [ "multi-user.target" ];
    after = [ "network.target" "caddy.service" "postgresql.service" ];
    serviceConfig = {
      Type = "simple";
      Environment = "PATH=/run/current-system/sw/bin:/nix/var/nix/profiles/default/bin";
      ExecStart = "${pkgs.nodejs_22}/bin/node /opt/preview-webhook/dist/index.js";
      Restart = "always";
      RestartSec = 5;
      EnvironmentFile = "/var/secrets/preview.env";
      WorkingDirectory = "/opt/preview-webhook";
    };
  };
}
