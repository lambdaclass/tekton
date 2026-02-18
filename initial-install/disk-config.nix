# Disko configuration for Hetzner dedicated server with 2x SSDs in RAID 1
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
            mdadm_boot = {
              size = "1G";
              content = {
                type = "mdraid";
                name = "boot";
              };
            };
            mdadm_root = {
              size = "100%";
              content = {
                type = "mdraid";
                name = "root";
              };
            };
          };
        };
      };
      disk1 = {
        type = "disk";
        device = "DISK_DEVICE_1";
        content = {
          type = "gpt";
          partitions = {
            boot = {
              size = "1M";
              type = "EF02"; # BIOS boot partition
            };
            mdadm_boot = {
              size = "1G";
              content = {
                type = "mdraid";
                name = "boot";
              };
            };
            mdadm_root = {
              size = "100%";
              content = {
                type = "mdraid";
                name = "root";
              };
            };
          };
        };
      };
    };
    mdadm = {
      boot = {
        type = "mdadm";
        level = 1;
        metadata = "1.0";
        content = {
          type = "filesystem";
          format = "ext4";
          mountpoint = "/boot";
        };
      };
      root = {
        type = "mdadm";
        level = 1;
        content = {
          type = "filesystem";
          format = "ext4";
          mountpoint = "/";
        };
      };
    };
  };
}
