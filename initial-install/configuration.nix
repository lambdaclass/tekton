# Initial NixOS configuration for nixos-anywhere installation
# UPDATE: IP addresses, gateway, and SSH key before use
{ modulesPath, lib, pkgs, ... }:
{
  imports = [
    (modulesPath + "/installer/scan/not-detected.nix")
  ];

  # Boot configuration - disko handles grub devices via EF02 partitions
  boot.loader.grub = {
    enable = true;
    efiSupport = false;
  };

  # Enable software RAID
  boot.swraid = {
    enable = true;
    mdadmConf = "MAILADDR root";
  };

  # Kernel modules for Hetzner hardware
  boot.initrd.availableKernelModules = [ INITRD_KERNEL_MODULES ];
  boot.kernelModules = [ KVM_KERNEL_MODULE ];

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
  ];

  system.stateVersion = "24.05";
}
