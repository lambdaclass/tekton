{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  };

  outputs = { self, nixpkgs, ... }:
  let
    system = "x86_64-linux";
    pkgs = import nixpkgs {
      inherit system;
      config.allowUnfree = true;
    };
  in {
    # Agent container configuration (built by `agent build`, used by `agent create`)
    nixosConfigurations.agent = nixpkgs.lib.nixosSystem {
      inherit system;
      modules = [
        { nixpkgs.pkgs = pkgs; }
        ./agent-config.nix
      ];
    };

  };
}
