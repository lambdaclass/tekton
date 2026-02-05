# Main NixOS configuration for Hetzner server with microvm support
# UPDATE: IP addresses, gateway, and SSH key before use
{ config, lib, pkgs, modulesPath, ... }:
{
  imports = [
    (modulesPath + "/installer/scan/not-detected.nix")
  ];

  # Boot configuration
  boot.loader.grub = {
    enable = true;
    efiSupport = false;
    devices = [ "/dev/sda" "/dev/sdb" ];
  };

  boot.swraid = {
    enable = true;
    mdadmConf = "MAILADDR root";
  };

  boot.initrd.availableKernelModules = [ "ahci" "sd_mod" "r8169" ];
  boot.kernelModules = [ "kvm-intel" ];

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

    nameservers = [ "185.12.64.1" "185.12.64.2" ];

    # Enable NAT for microvms
    nat = {
      enable = true;
      internalInterfaces = [ "microbr" ];
      externalInterface = "enp3s0";
    };
  };

  # SSH
  services.openssh = {
    enable = true;
    settings = {
      PermitRootLogin = "prohibit-password";
      PasswordAuthentication = false;
    };
  };

  # UPDATE: Your SSH public key
  users.users.root.openssh.authorizedKeys.keys = [
    "ssh-ed25519 AAAA... your-key-here"
  ];

  environment.systemPackages = with pkgs; [
    vim
    git
    curl
    htop
    tmux
    claude-code  # For initial credential setup on host
  ];

  system.stateVersion = "24.11";
}
