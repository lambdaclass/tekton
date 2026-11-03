# Disko configuration for a single-disk server without RAID (e.g. Hetzner Cloud).
# setup.sh uses this instead of disk-config.nix when it finds exactly one disk.
# The labels are what server-config/configuration.nix mounts on these servers.
{ lib, ... }:
{
  disko.devices = {
    disk = {
      disk0 = {
        type = "disk";
        device = "DISK_DEVICE_0";
        content = {
          type = "gpt";
          partitions = {
            boot = {
              size = "1M";
              type = "EF02"; # BIOS boot partition
            };
            bootfs = {
              size = "1G";
              content = {
                type = "filesystem";
                format = "ext4";
                extraArgs = [ "-L" "nixos-boot" ];
                mountpoint = "/boot";
              };
            };
            root = {
              size = "100%";
              content = {
                type = "filesystem";
                format = "ext4";
                extraArgs = [ "-L" "nixos-root" ];
                mountpoint = "/";
              };
            };
          };
        };
      };
    };
  };
}
