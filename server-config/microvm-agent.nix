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
  mkMicrovm = import ./lib/mkMicrovm.nix { inherit pkgs lib; };
in
mkMicrovm {
  vmName = "agent1";
  vmIP = "192.168.83.10";
  vmMAC = "02:00:00:00:00:10";
}
