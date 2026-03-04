# NixOS configuration for imperative nspawn agent containers
# Built via: nix build /etc/nixos#nixosConfigurations.agent.config.system.build.toplevel
# Used by: nixos-container create <name> --system-path <closure>
#
# Credentials flow:
# 1. Host stores long-lived OAuth token at /var/secrets/claude/oauth_token
# 2. `agent create` copies /var/secrets/claude/ to /mnt/claude-creds in the container
# 3. At boot, CLAUDE_CODE_OAUTH_TOKEN is set via /etc/profile.d/ for all sessions
# 4. Claude authenticates via env var — no credential files needed
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

  # Set up credentials at startup
  systemd.services.setup-claude-creds = {
    description = "Set up Claude credentials and git config";
    wantedBy = [ "multi-user.target" ];
    before = [ "multi-user.target" ];
    serviceConfig = {
      Type = "oneshot";
      RemainAfterExit = true;
    };
    script = ''
      # Write OAuth token to profile so every SSH session gets it
      mkdir -p /etc/profile.d
      if [ -f /mnt/claude-creds/oauth_token ]; then
        TOKEN=$(cat /mnt/claude-creds/oauth_token | tr -d '[:space:]')
        cat > /etc/profile.d/claude-token.sh << EOF
export CLAUDE_CODE_OAUTH_TOKEN=$TOKEN
EOF
        chmod 644 /etc/profile.d/claude-token.sh
      fi
    '';
  };

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
    openssh.authorizedKeys.keys = [
      "ssh-ed25519 AAAA... your-key-here"
      "ssh-ed25519 AAAA... root-key-here"
    ];
  };

  security.sudo.wheelNeedsPassword = false;

  users.users.root = {
    openssh.authorizedKeys.keys = [
      "ssh-ed25519 AAAA... your-key-here"
      "ssh-ed25519 AAAA... root-key-here"
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
