# MicroVM host configuration - sets up bridge networking
{ config, lib, pkgs, ... }:
{
  # Bridge network for microvms
  systemd.network = {
    enable = true;
    netdevs."10-microbr" = {
      netdevConfig = {
        Kind = "bridge";
        Name = "microbr";
      };
    };
    networks."10-microbr" = {
      matchConfig.Name = "microbr";
      networkConfig.Address = "192.168.83.1/24";
    };
    networks."11-microvm" = {
      matchConfig.Name = "vm-*";
      networkConfig.Bridge = "microbr";
    };
  };

  # Enable IP forwarding for NAT
  boot.kernel.sysctl."net.ipv4.ip_forward" = 1;

  # Don't autostart VMs by default
  microvm.autostart = [];
}
