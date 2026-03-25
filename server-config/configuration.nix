# Main NixOS configuration for Hetzner dedicated server with RAID 1
# This is for fresh installs via nixos-anywhere on Hetzner dedicated servers.
# For existing NixOS servers, import tekton-services.nix directly instead.
#
# UPDATE: IP addresses, gateway, disk devices, and SSH key before use
{ config, lib, pkgs, modulesPath, ... }:
{
  imports = [
    (modulesPath + "/installer/scan/not-detected.nix")
    ./tekton-services.nix
  ];

  # Boot configuration (Hetzner RAID 1)
  boot.loader.grub = {
    enable = true;
    efiSupport = false;
    devices = [ "DISK_DEVICE_0" "DISK_DEVICE_1" ];
  };

  boot.swraid = {
    enable = true;
    mdadmConf = "MAILADDR root";
  };

  boot.initrd.availableKernelModules = [ INITRD_KERNEL_MODULES ];

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

    nameservers = [ "1.1.1.1" "1.0.0.1" ];
  };

  # UPDATE: Your SSH public key
  users.users.root.openssh.authorizedKeys.keys = [
    "ssh-ed25519 AAAA... your-key-here"
  ];

  system.stateVersion = "24.11";
}
