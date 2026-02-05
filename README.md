# NixOS MicroVM Setup for Claude Code Agents on Hetzner

This guide sets up a Hetzner bare metal server with NixOS and microvms for running Claude Code agents in isolated, ephemeral environments.

## Directory Structure

```
nixos-hetzner-microvm/
├── README.md                    # This file
├── initial-install/             # Used once for nixos-anywhere installation
│   ├── flake.nix
│   ├── disk-config.nix          # RAID 1 across two SSDs
│   └── configuration.nix
├── server-config/               # Copied to /etc/nixos after install
│   ├── flake.nix
│   ├── configuration.nix
│   ├── microvm-host.nix         # Bridge networking setup
│   ├── microvm-agent.nix        # MicroVM definition (uses mkMicrovm)
│   └── lib/
│       └── mkMicrovm.nix        # Reusable microvm factory function
├── scripts/
│   └── deploy-vm.sh             # Helper to deploy a VM to the server
└── .claude/
    └── skills/
        └── create-microvm/
            └── SKILL.md          # Claude skill for creating new VMs
```

## Prerequisites

- Local machine with Nix installed (flakes enabled)
- SSH key pair
- Hetzner account

## Part 1: Provision Hetzner Server

1. Order a dedicated server at [Hetzner Robot](https://robot.hetzner.com) (e.g., AX41-NVMe)
2. Wait for provisioning, note your server IP
3. Activate rescue mode: Server → Rescue tab → Activate Linux 64-bit
4. Reset the server: Reset tab → Hardware reset
5. SSH into rescue: `ssh root@YOUR_SERVER_IP`

Gather network info while in rescue mode:
```bash
ip addr show        # Note interface name (e.g., enp3s0)
ip route            # Note gateway
```

## Part 2: Configure and Install

### 2.1 Update Configuration Files

Edit `initial-install/configuration.nix`:
- Set your server's IP address
- Set your gateway
- Add your SSH public key

### 2.2 Run nixos-anywhere

From your local machine (server must be in rescue mode):

```bash
cd initial-install
nix run github:nix-community/nixos-anywhere -- \
  --flake '.#hetzner-dedicated' \
  root@YOUR_SERVER_IP
```

Wait ~5-10 minutes. Server reboots automatically when done.

### 2.3 Verify Installation

```bash
ssh root@YOUR_SERVER_IP
cat /etc/os-release    # Should show NixOS
cat /proc/mdstat       # Should show RAID arrays syncing
```

## Part 3: Set Up MicroVMs

### 3.1 Copy Server Config

```bash
# From your local machine
scp -r server-config/. root@YOUR_SERVER_IP:/etc/nixos/
```

### 3.2 Create Secrets Directory

```bash
ssh root@YOUR_SERVER_IP

# Create directory for Claude credentials
mkdir -p /var/secrets/claude
chmod 755 /var/secrets
chmod 755 /var/secrets/claude

# Apply configuration
cd /etc/nixos
nixos-rebuild switch
```

### 3.3 Set Up Claude Credentials (One-Time)

On the host, authenticate Claude. Credentials are stored in a shared directory that microvms can read:

```bash
export CLAUDE_CONFIG_DIR=/var/secrets/claude
claude login
```

Complete OAuth in your browser. This creates `.claude.json` and `.credentials.json` inside `/var/secrets/claude/`.

### 3.4 Fix Permissions for Credentials

After login, fix ownership so microvms can read the credentials:

```bash
chown -R microvm:kvm /var/secrets/claude
chmod -R 755 /var/secrets/claude
```

**How it works:** The microvm mounts `/var/secrets/claude` as a read-only 9p share at `/mnt/claude-creds`. At boot, a systemd service copies these credentials to `/home/agent/.claude/` and sets `CLAUDE_CONFIG_DIR=/home/agent/.claude` for the agent user. This allows Claude to work without re-authenticating.

## Part 4: Using MicroVMs

### Start a VM

```bash
systemctl start microvm@agent1
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
systemctl stop microvm@agent1
```

## Adding More MicroVMs

### Using the Claude Skill (Recommended)

The easiest way to create a new VM is with the Claude Code skill. In a Claude Code session inside this repo, say:

> Create a new microvm called "agent2" for working on my frontend project

Claude will:
1. Scan existing VMs to pick the next available IP/MAC
2. Create `server-config/microvm-agent2.nix` using the `mkMicrovm` factory
3. Update `server-config/flake.nix` to include the new module
4. Commit the changes
5. Provide deployment instructions

### Using the Deploy Script

After creating the VM config (manually or via the skill), deploy it:

```bash
# Basic deployment
./scripts/deploy-vm.sh agent2 YOUR_SERVER_IP

# Deploy and clone a repo into the workspace
./scripts/deploy-vm.sh agent2 YOUR_SERVER_IP --clone https://github.com/user/repo.git
```

### Manual Approach

Create a file like `server-config/microvm-agent2.nix`:

```nix
{ config, lib, pkgs, ... }:
let
  mkMicrovm = import ./lib/mkMicrovm.nix { inherit pkgs lib; };
in
mkMicrovm {
  vmName = "agent2";
  vmIP = "192.168.83.11";
  vmMAC = "02:00:00:00:00:0b";
  workspacePath = "/var/lib/microvms/agent2/workspace";
  sshHostKeysPath = "/var/lib/microvms/agent2/ssh-host-keys";
  # extraPackages = with pkgs; [ protobuf terraform ];
}
```

Add it to `flake.nix` modules list, then deploy with `scp` + `nixos-rebuild switch`.

### mkMicrovm Parameters

| Parameter | Required | Default | Description |
|-----------|----------|---------|-------------|
| `vmName` | yes | — | VM identifier (used as hostname, tap interface name) |
| `vmIP` | yes | — | Static IP in 192.168.83.0/24 |
| `vmMAC` | yes | — | Unique MAC address |
| `workspacePath` | no | null | Host directory shared as `/home/agent/workspace` in VM |
| `sshHostKeysPath` | no | null | Host directory for persisting SSH host keys |
| `extraPackages` | no | [] | Additional Nix packages |
| `vcpu` | no | 4 | Number of virtual CPUs |
| `mem` | no | 4096 | Memory in MB |
| `varImgSize` | no | 8192 | Size of /var volume image in MB |

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
export CLAUDE_CONFIG_DIR=/var/secrets/claude
claude login
chown -R microvm:kvm /var/secrets/claude
chmod -R 755 /var/secrets/claude

# Verify credentials in VM
ls -la /home/agent/.claude/
echo $CLAUDE_CONFIG_DIR  # Should show /home/agent/.claude
```

## References

- [microvm.nix](https://github.com/astro/microvm.nix)
- [nixos-anywhere](https://github.com/nix-community/nixos-anywhere)
- [disko](https://github.com/nix-community/disko)
- [Michael Stapelberg's Coding Agent MicroVM Post](https://michael.stapelberg.ch/posts/2026-02-01-coding-agent-microvm-nix/)
