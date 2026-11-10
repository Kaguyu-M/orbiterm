{
  description = "Orbiterm — terminal et client d'accès distant (SSH, SFTP, RDP, VNC…)";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs =
    { self, nixpkgs }:
    let
      systems = [ "x86_64-linux" "aarch64-linux" ];
      forAll = f: nixpkgs.lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});
    in
    {
      packages = forAll (pkgs: rec {
        orbiterm = pkgs.callPackage ./nix/package.nix { };
        default = orbiterm;
      });

      apps = forAll (pkgs: {
        default = {
          type = "app";
          program = "${self.packages.${pkgs.stdenv.hostPlatform.system}.default}/bin/orbiterm";
        };
      });

      overlays.default = final: _prev: { orbiterm = final.callPackage ./nix/package.nix { }; };

      # NixOS : `imports = [ orbiterm.nixosModules.default ]; programs.orbiterm.enable = true;`
      nixosModules.default =
        { config, lib, pkgs, ... }:
        {
          options.programs.orbiterm.enable = lib.mkEnableOption "Orbiterm";
          config = lib.mkIf config.programs.orbiterm.enable {
            environment.systemPackages = [ self.packages.${pkgs.stdenv.hostPlatform.system}.default ];
          };
        };
    };
}
