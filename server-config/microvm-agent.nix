# MicroVM configuration for Claude Code agent
# UPDATE: SSH key before use
#
# Credentials flow:
# 1. Host stores Claude credentials at /var/secrets/claude/ (via CLAUDE_CONFIG_DIR)
# 2. This directory is mounted read-only at /mnt/claude-creds in the VM
# 3. At boot, credentials are copied to /home/agent/.claude/
# 4. CLAUDE_CONFIG_DIR is set to /home/agent/.claude for all users
{ config, lib, pkgs, ... }:
let
  vmName = "agent1";
  vmIP = "192.168.83.10";
  vmMAC = "02:00:00:00:00:10";
in
{
  microvm.vms.${vmName} = {
    autostart = false;

    config = { config, pkgs, ... }: {
      networking.hostName = vmName;

      # Network configuration with DNS
      systemd.network = {
        enable = true;
        networks."20-lan" = {
          matchConfig.Type = "ether";
          networkConfig = {
            Address = "${vmIP}/24";
            Gateway = "192.168.83.1";
          };
        };
      };

      services.resolved = {
        enable = true;
        fallbackDns = [ "8.8.8.8" "1.1.1.1" ];
      };
      networking.nameservers = [ "8.8.8.8" "1.1.1.1" ];

      # MicroVM settings
      microvm = {
        hypervisor = "qemu";
        vcpu = 4;
        mem = 4096;

        interfaces = [{
          type = "tap";
          id = "vm-${vmName}";
          mac = vmMAC;
        }];

        shares = [
          {
            tag = "ro-store";
            source = "/nix/store";
            mountPoint = "/nix/.ro-store";
          }
          {
            # Read-only credentials from host
            tag = "claude-creds";
            source = "/var/secrets/claude";
            mountPoint = "/mnt/claude-creds";
          }
        ];

        writableStoreOverlay = "/nix/.rw-store";

        volumes = [{
          image = "var.img";
          mountPoint = "/var";
          size = 8192;
        }];
      };

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
          # Copy config files INSIDE .claude directory (not in HOME)
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

      # Agent user for running Claude (can't run --dangerously-skip-permissions as root)
      users.users.agent = {
        isNormalUser = true;
        home = "/home/agent";
        shell = pkgs.bash;
        extraGroups = [ "wheel" ];
        # UPDATE: Your SSH public key
        openssh.authorizedKeys.keys = [
          "ssh-ed25519 AAAA... your-key-here"
        ];
      };

      security.sudo.wheelNeedsPassword = false;

      users.users.root = {
        password = "changeme";  # Fallback password
        # UPDATE: Your SSH public key
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
    };
  };
}
