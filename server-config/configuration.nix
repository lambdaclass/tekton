# Main NixOS configuration for Hetzner server with nspawn agent containers
# UPDATE: IP addresses, gateway, and SSH key before use
{ config, lib, pkgs, modulesPath, ... }:
{
  imports = [
    (modulesPath + "/installer/scan/not-detected.nix")
  ];

  # Boot configuration
  boot.loader.grub = {
    enable = true;
    efiSupport = false;
    devices = [ "/dev/sda" "/dev/sdb" ];
  };

  boot.swraid = {
    enable = true;
    mdadmConf = "MAILADDR root";
  };

  boot.initrd.availableKernelModules = [ "ahci" "sd_mod" "r8169" ];

  # Filesystems (created by disko during initial install)
  fileSystems."/" = {
    device = "/dev/md/root";
    fsType = "ext4";
  };

  fileSystems."/boot" = {
    device = "/dev/md/boot";
    fsType = "ext4";
  };

  # Network configuration - UPDATE THESE FOR YOUR SERVER
  networking = {
    hostName = "nixos-server";
    useDHCP = false;

    interfaces.enp3s0 = {
      ipv4.addresses = [{
        address = "YOUR.SERVER.IP.HERE";  # <-- YOUR SERVER IP
        prefixLength = 26;
      }];
    };

    defaultGateway = {
      address = "YOUR.GATEWAY.IP.HERE";    # <-- YOUR GATEWAY
      interface = "enp3s0";
    };

    nameservers = [ "185.12.64.1" "185.12.64.2" ];

    # Enable NAT for agent containers (ve-+ matches all container veth interfaces)
    nat = {
      enable = true;
      internalInterfaces = [ "ve-+" ];
      externalInterface = "enp3s0";
    };
  };

  # SSH
  services.openssh = {
    enable = true;
    settings = {
      PermitRootLogin = "prohibit-password";
      PasswordAuthentication = false;
    };
  };

  # UPDATE: Your SSH public key
  users.users.root.openssh.authorizedKeys.keys = [
    "ssh-ed25519 AAAA... your-key-here"
  ];

  # Enable IP forwarding for container NAT
  boot.kernel.sysctl."net.ipv4.ip_forward" = 1;

  # Required for imperative nixos-container create/start/stop
  boot.enableContainers = true;

  # Enable flakes (needed by `agent build` to evaluate the agent flake output)
  nix.settings.experimental-features = [ "nix-command" "flakes" ];

  # PostgreSQL for preview deployments
  services.postgresql = {
    enable = true;
    enableTCPIP = true;
    settings = {
      listen_addresses = "*";
    };
    authentication = ''
      local all all peer
      host all all 10.100.0.0/24 md5
    '';
  };

  # Caddy reverse proxy for preview deployments (auto HTTPS via Let's Encrypt)
  services.caddy = {
    enable = true;
    extraConfig = ''
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

  # Firewall: allow HTTP (ACME challenges) and HTTPS
  networking.firewall.allowedTCPPorts = [ 80 443 ];

  # Trust container veth interfaces (allows containers to reach host PostgreSQL, etc.)
  networking.firewall.trustedInterfaces = [ "ve-+" ];

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

  system.stateVersion = "24.11";
}
