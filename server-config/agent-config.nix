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
      if [ -f /mnt/claude-creds/oauth_token ]; then
        TOKEN=$(cat /mnt/claude-creds/oauth_token | tr -d '[:space:]')
        cat > /etc/profile.d/claude-token.sh << EOF
export CLAUDE_CODE_OAUTH_TOKEN=$TOKEN
EOF
        chmod 644 /etc/profile.d/claude-token.sh
      fi

      # Copy SSH signing key for signed git commits
      mkdir -p /home/agent/.ssh
      cp /mnt/claude-creds/signing_key /home/agent/.ssh/signing_key 2>/dev/null || true
      chown -R agent:users /home/agent/.ssh
      chmod 700 /home/agent/.ssh
      chmod 600 /home/agent/.ssh/signing_key 2>/dev/null || true

      # Configure git to sign commits with SSH key
      cat > /home/agent/.gitconfig << 'GITEOF'
[user]
	name = Claude (Dashboard)
	email = jrchatruc@gmail.com
[gpg]
	format = ssh
[user]
	signingkey = /home/agent/.ssh/signing_key
[commit]
	gpgsign = true
GITEOF
      chown agent:users /home/agent/.gitconfig
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
      "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIEWMu5wyCIJclVNVk3Judmu5zkWxkbtTJrcC0BpEcVfy jrchatruc@gmail.com"
      "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIBIrWQgyW2acM35arp+DVr8Jo5S7A4vqbP9gLk3pMRhw root@nixos-server"
    ];
  };

  security.sudo.wheelNeedsPassword = false;

  users.users.root = {
    password = "changeme";  # Fallback password
    openssh.authorizedKeys.keys = [
      "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIEWMu5wyCIJclVNVk3Judmu5zkWxkbtTJrcC0BpEcVfy jrchatruc@gmail.com"
      "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIBIrWQgyW2acM35arp+DVr8Jo5S7A4vqbP9gLk3pMRhw root@nixos-server"
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
