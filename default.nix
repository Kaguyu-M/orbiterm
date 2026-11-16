# Orbiterm sans flake : `nix-build`, ou depuis /etc/nixos/configuration.nix (voir nix/module.nix).
{
  pkgs ? import <nixpkgs> { },
}:
pkgs.callPackage ./nix/package.nix { }
