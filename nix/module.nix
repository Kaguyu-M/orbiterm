# Module NixOS d'Orbiterm, utilisable avec ou sans flake.
#
# Sans flake, dans /etc/nixos/configuration.nix :
#
#   { pkgs, ... }:
#   let
#     orbiterm = builtins.fetchTarball "https://github.com/Kaguyu-M/orbiterm/archive/main.tar.gz";
#   in
#   {
#     imports = [ "${orbiterm}/nix/module.nix" ];
#     programs.orbiterm.enable = true;
#   }
{
  config,
  lib,
  pkgs,
  ...
}:
let
  cfg = config.programs.orbiterm;
in
{
  options.programs.orbiterm = {
    enable = lib.mkEnableOption "Orbiterm, terminal et client d'accès distant (SSH, SFTP, RDP, VNC…)";
    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.callPackage ./package.nix { };
      defaultText = lib.literalExpression "pkgs.callPackage ./package.nix { }";
      description = "Paquet Orbiterm à installer.";
    };
  };

  config = lib.mkIf cfg.enable {
    environment.systemPackages = [ cfg.package ];
  };
}
