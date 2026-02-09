# NixOS configuration for imperative nspawn agent containers
# Built via: nix build /etc/nixos#nixosConfigurations.agent.config.system.build.toplevel
# Used by: nixos-container create <name> --system-path <closure>
#
# Credentials flow:
# 1. Host stores Claude credentials at /var/secrets/claude/
# 2. `agent create` copies them to /mnt/claude-creds in the container filesystem
# 3. At boot, credentials are copied to /home/agent/.claude/
# 4. CLAUDE_CONFIG_DIR is set to /home/agent/.claude for all users
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

  # Copy credentials at startup to local writable directory
  systemd.services.setup-claude-creds = {
    description = "Copy Claude credentials to local directory";
    wantedBy = [ "multi-user.target" ];
    before = [ "multi-user.target" ];
    serviceConfig = {
      Type = "oneshot";
      RemainAfterExit = true;
    };
    script = ''
      mkdir -p /home/agent/.claude
      # Copy config files
      cp /mnt/claude-creds/.claude.json /home/agent/.claude/ 2>/dev/null || true
      cp /mnt/claude-creds/.credentials.json /home/agent/.claude/ 2>/dev/null || true
      # Copy subdirectories
      cp -r /mnt/claude-creds/cache /home/agent/.claude/ 2>/dev/null || true
      cp -r /mnt/claude-creds/statsig /home/agent/.claude/ 2>/dev/null || true
      chown -R agent:users /home/agent/.claude
      chmod -R 700 /home/agent/.claude
    '';
  };

  # Set CLAUDE_CONFIG_DIR for the agent user
  environment.variables.CLAUDE_CONFIG_DIR = "/home/agent/.claude";

  # SSH access
  services.openssh = {
    enable = true;
    settings.PermitRootLogin = "yes";
  };

  # Agent user for running Claude
  users.users.agent = {
    isNormalUser = true;
    home = "/home/agent";
    shell = pkgs.bash;
    extraGroups = [ "wheel" ];
    # UPDATE: Your SSH public key (substituted by setup.sh)
    openssh.authorizedKeys.keys = [
      "ssh-ed25519 AAAA... your-key-here"
    ];
  };

  security.sudo.wheelNeedsPassword = false;

  users.users.root = {
    password = "changeme";  # Fallback password
    openssh.authorizedKeys.keys = [
      "ssh-ed25519 AAAA... your-key-here"
    ];
  };

  # Development packages
  environment.systemPackages = with pkgs; [
    vim
    git
    curl
    wget
    htop
    tmux
    ripgrep
    fd
    jq
    nodejs_22
    python3
    go
    rustup
    gcc
    gnumake
    claude-code
  ];

  system.stateVersion = "24.11";
}
