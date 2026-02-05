# Reusable factory function for creating microvm definitions.
# Usage:
#   let mkMicrovm = import ./lib/mkMicrovm.nix { inherit pkgs lib; };
#   in mkMicrovm { vmName = "agent1"; vmIP = "192.168.83.10"; vmMAC = "02:00:00:00:00:10"; }
{ pkgs, lib }:

{
  vmName,
  vmIP,
  vmMAC,
  workspacePath ? null,
  sshHostKeysPath ? null,
  extraPackages ? [],
  vcpu ? 4,
  mem ? 4096,
  varImgSize ? 8192,
}:

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
        inherit vcpu mem;

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
        ]
        ++ lib.optional (workspacePath != null) {
          tag = "workspace";
          source = workspacePath;
          mountPoint = "/home/agent/workspace";
        }
        ++ lib.optional (sshHostKeysPath != null) {
          tag = "ssh-host-keys";
          source = sshHostKeysPath;
          mountPoint = "/mnt/ssh-host-keys";
        };

        writableStoreOverlay = "/nix/.rw-store";

        volumes = [{
          image = "var.img";
          mountPoint = "/var";
          size = varImgSize;
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

      # Persist SSH host keys if a shared directory is provided
      systemd.services.setup-ssh-host-keys = lib.mkIf (sshHostKeysPath != null) {
        description = "Restore or generate persistent SSH host keys";
        wantedBy = [ "multi-user.target" ];
        before = [ "sshd.service" ];
        serviceConfig = {
          Type = "oneshot";
          RemainAfterExit = true;
        };
        script = ''
          # If host keys exist in shared dir, copy them in
          if [ -f /mnt/ssh-host-keys/ssh_host_ed25519_key ]; then
            cp /mnt/ssh-host-keys/ssh_host_* /etc/ssh/
            chmod 600 /etc/ssh/ssh_host_*_key
            chmod 644 /etc/ssh/ssh_host_*_key.pub
          else
            # Generate and persist keys back to shared dir
            ssh-keygen -t ed25519 -f /etc/ssh/ssh_host_ed25519_key -N "" -q
            ssh-keygen -t rsa -b 4096 -f /etc/ssh/ssh_host_rsa_key -N "" -q
            cp /etc/ssh/ssh_host_* /mnt/ssh-host-keys/ 2>/dev/null || true
          fi
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
        openssh.authorizedKeys.keys = [
          "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIEWMu5wyCIJclVNVk3Judmu5zkWxkbtTJrcC0BpEcVfy jrchatruc@gmail.com"
        ];
      };

      security.sudo.wheelNeedsPassword = false;

      users.users.root = {
        password = "changeme";  # Fallback password
        openssh.authorizedKeys.keys = [
          "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIEWMu5wyCIJclVNVk3Judmu5zkWxkbtTJrcC0BpEcVfy jrchatruc@gmail.com"
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
      ] ++ extraPackages;

      system.stateVersion = "24.11";
    };
  };
}
