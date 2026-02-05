# Skill: Create a New MicroVM

This skill creates a new microvm definition for running Claude Code agents on the NixOS server. The server is already set up with NixOS + microvm.nix host support.

## When to Use

Use this skill when the user asks to:
- Create a new microvm / VM / agent environment
- Set up an isolated coding workspace
- Add another Claude Code agent instance

## Steps

### 1. Determine VM Parameters

Ask the user for:
- **VM name** — a short identifier (e.g., "agent2", "codesearch", "frontend"). Must be valid as a hostname (lowercase, no spaces, alphanumeric + hyphens).
- **Purpose / extra packages** — what the VM is for, so you can add relevant packages (e.g., protobuf, docker, terraform). Default base packages (git, nodejs, python3, go, rustup, gcc, etc.) are already included.
- **Workspace** — whether to set up a shared workspace directory on the host (recommended). If yes, it will be at `/var/lib/microvms/<vmName>/workspace` on the host, mounted at `/home/agent/workspace` in the VM.
- **SSH host key persistence** — whether to persist SSH host keys across VM restarts (recommended). If yes, keys are stored at `/var/lib/microvms/<vmName>/ssh-host-keys` on the host.

### 2. Scan Existing VMs and Pick Next Available IP/MAC

Read all `server-config/microvm-*.nix` files to find which IPs and MACs are already in use.

IP addresses are in the `192.168.83.0/24` subnet, starting at `.10`. MAC addresses follow the pattern `02:00:00:00:00:XX` where XX corresponds to the last octet of the IP (in hex).

Pick the next sequential IP and MAC that are not already taken. For example, if `.10` and `.11` are used, pick `.12` → IP `192.168.83.12`, MAC `02:00:00:00:00:0c`.

### 3. Create the VM Configuration File

Create `server-config/microvm-<vmName>.nix`:

```nix
# MicroVM configuration for <vmName>
{ config, lib, pkgs, ... }:
let
  mkMicrovm = import ./lib/mkMicrovm.nix { inherit pkgs lib; };
in
mkMicrovm {
  vmName = "<vmName>";
  vmIP = "<chosen IP>";
  vmMAC = "<chosen MAC>";
  # Include these only if the user wants them:
  workspacePath = "/var/lib/microvms/<vmName>/workspace";
  sshHostKeysPath = "/var/lib/microvms/<vmName>/ssh-host-keys";
  # Include only if user needs extra packages beyond the defaults:
  extraPackages = with pkgs; [ /* user-requested packages */ ];
}
```

### 4. Update flake.nix

Add the new module to the `modules` list in `server-config/flake.nix`:

```nix
modules = [
  { nixpkgs.pkgs = pkgs; }
  microvm.nixosModules.host
  ./configuration.nix
  ./microvm-host.nix
  ./microvm-agent.nix
  ./microvm-<vmName>.nix   # <-- add this line
];
```

### 5. Provide Deployment Instructions

After creating the files, tell the user to deploy with the helper script:

```bash
# From the repo root:
./scripts/deploy-vm.sh <vmName> <server-ip>
```

Or provide manual deployment steps:

```bash
# 1. Copy config to server
scp -r server-config/. root@<SERVER_IP>:/etc/nixos/

# 2. SSH into the server
ssh root@<SERVER_IP>

# 3. Create workspace and SSH key directories on the host (if using shared dirs)
mkdir -p /var/lib/microvms/<vmName>/workspace
mkdir -p /var/lib/microvms/<vmName>/ssh-host-keys

# 4. Rebuild NixOS configuration
cd /etc/nixos && nixos-rebuild switch

# 5. Start the VM
systemctl start microvm@<vmName>

# 6. (Optional) Clone repos into the workspace
cd /var/lib/microvms/<vmName>/workspace
git clone <repo-url>

# 7. SSH into the VM
ssh -J root@<SERVER_IP> agent@<vmIP>

# 8. Run Claude inside the VM
claude --dangerously-skip-permissions
```

### 6. Commit the Changes

Stage and commit the new/modified files:
- `server-config/microvm-<vmName>.nix` (new)
- `server-config/flake.nix` (modified)

Use a descriptive commit message like: `Add microvm definition for <vmName>`

## Reference

- Factory function: `server-config/lib/mkMicrovm.nix`
- Existing VM example: `server-config/microvm-agent.nix`
- Host networking: `server-config/microvm-host.nix`
- Deploy script: `scripts/deploy-vm.sh`
- Subnet: `192.168.83.0/24`, gateway `192.168.83.1`
- IP range for VMs: `.10` and up
- MAC pattern: `02:00:00:00:00:XX`
