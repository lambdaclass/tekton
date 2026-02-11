{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  };

  outputs = { self, nixpkgs, ... }:
  let
    system = "x86_64-linux";
    pkgs = import nixpkgs {
      inherit system;
      config.allowUnfree = true;  # Required for Claude Code
    };
  in {
    # Host server configuration
    nixosConfigurations.nixos-server = nixpkgs.lib.nixosSystem {
      inherit system;
      modules = [
        { nixpkgs.pkgs = pkgs; }
        ./configuration.nix
      ];
    };

    # Agent container configuration (built by `agent build`, used by `agent create`)
    nixosConfigurations.agent = nixpkgs.lib.nixosSystem {
      inherit system;
      modules = [
        { nixpkgs.pkgs = pkgs; }
        ./agent-config.nix
      ];
    };

    # Preview container configuration (built by `preview build`, used by `preview create`)
    nixosConfigurations.preview = nixpkgs.lib.nixosSystem {
      inherit system;
      modules = [
        { nixpkgs.pkgs = pkgs; }
        ./preview-config.nix
      ];
    };
  };
}
