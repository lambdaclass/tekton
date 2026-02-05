# NixOS MicroVM Setup for Claude Code Agents on Hetzner

> Based on [Michael Stapelberg's blog post](https://michael.stapelberg.ch/posts/2026-02-01-coding-agent-microvm-nix/) on running coding agents in NixOS MicroVMs. Uses [nixos-anywhere](https://github.com/nix-community/nixos-anywhere) for remote NixOS installation and [microvm.nix](https://github.com/astro/microvm.nix) for lightweight VM management.

This repo sets up a Hetzner bare metal server with NixOS and MicroVMs for running Claude Code agents in isolated, ephemeral environments.

## Directory Structure

```
nixos-claude/
├── README.md
├── setup.sh                     # Automated setup script
├── initial-install/             # Used once for nixos-anywhere installation
│   ├── flake.nix
│   ├── disk-config.nix          # RAID 1 across two SSDs
│   └── configuration.nix
└── server-config/               # Copied to /etc/nixos after install
    ├── flake.nix
    ├── configuration.nix
    ├── microvm-host.nix         # Bridge networking setup
    └── microvm-agent.nix        # MicroVM definition
```

## Prerequisites

- Local machine with [Nix installed](https://nixos.org/download/) (flakes enabled)
- SSH key pair (`ssh-keygen` if you don't have one)
- Hetzner account

## Setup

### Step 1: Provision Hetzner Server

1. Order a dedicated server at [Hetzner Robot](https://robot.hetzner.com) (e.g., AX41-NVMe)
2. Wait for provisioning, note your server IP
3. Activate rescue mode: Server → Rescue tab → Activate Linux 64-bit
4. Reset the server: Reset tab → Hardware reset
5. Wait a minute for the server to boot into rescue mode

### Step 2: Run the Setup Script

```bash
./setup.sh
```

The script will:
1. **Prompt** for your server IP and SSH public key
2. **Auto-detect** the gateway, network interface, and prefix length by SSHing into rescue mode
3. **Install NixOS** via nixos-anywhere (takes ~5-10 minutes)
4. **Configure the server** with MicroVM support and copy the server config
5. **Run `claude login`** interactively so you can authenticate via OAuth

No manual editing of configuration files is needed - the script handles all substitutions on temporary copies and leaves the repo files untouched.

## Using MicroVMs

### Start a VM

```bash
ssh root@YOUR_SERVER_IP 'systemctl start microvm@agent1'
```

### SSH into the VM

From your local machine (uses SSH jump, no password needed):
```bash
ssh -J root@YOUR_SERVER_IP agent@192.168.83.10
```

### Run Claude

```bash
# Regular mode
claude

# Skip all permission prompts (for automation)
claude --dangerously-skip-permissions
```

### Stop the VM

```bash
ssh root@YOUR_SERVER_IP 'systemctl stop microvm@agent1'
```

## Adding More MicroVMs

Create additional files like `microvm-agent2.nix` with different:
- `vmName` (e.g., "agent2")
- `vmIP` (e.g., "192.168.83.11")
- `vmMAC` (e.g., "02:00:00:00:00:11")

Add to `flake.nix` modules list, then `nixos-rebuild switch` on the server.

## Troubleshooting

| Problem | Solution |
|---------|----------|
| VM fails with "permission denied" on start | `chmod 755 /var/secrets/claude` on host |
| SSH host key warning after VM restart | `ssh-keygen -R 192.168.83.10` |
| Claude "Invalid API key" in VM | Re-run `claude login` on host with `CLAUDE_CONFIG_DIR=/var/secrets/claude`, fix permissions, restart VM |
| Claude permission errors in VM | `chown -R microvm:kvm /var/secrets/claude && chmod -R 755 /var/secrets/claude` on host |
| systemd-networkd-wait-online timeout | Benign - ignore it |
| Claude hangs on startup | Check credentials exist: `ls /home/agent/.claude/` in VM |
| Claude shows onboarding screen | Credentials not copied - check `/mnt/claude-creds/` is mounted and has files |

## Quick Reference

```bash
# VM management
systemctl start microvm@agent1
systemctl stop microvm@agent1
systemctl status microvm@agent1

# View logs
journalctl -u microvm@agent1 -f

# SSH into VM
ssh -J root@YOUR_SERVER_IP agent@192.168.83.10

# Rebuild after config changes
cd /etc/nixos && nixos-rebuild switch

# Re-authenticate Claude (on host)
CLAUDE_CONFIG_DIR=/var/secrets/claude claude login
chown -R microvm:kvm /var/secrets/claude
chmod -R 755 /var/secrets/claude

# Verify credentials in VM
ls -la /home/agent/.claude/
echo $CLAUDE_CONFIG_DIR  # Should show /home/agent/.claude
```
