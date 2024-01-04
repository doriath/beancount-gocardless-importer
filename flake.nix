{
  description = "Flake for beancount-gocardless-importer";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs";
    rust-overlay.url = "github:oxalica/rust-overlay";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, flake-utils, rust-overlay, ... }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs {
          inherit system overlays;
        };
        manifest = (pkgs.lib.importTOML ./Cargo.toml).package;
        rust = pkgs.rust-bin.stable.latest;
        rustPlatform = pkgs.recurseIntoAttrs (pkgs.makeRustPlatform {
          rustc = rust.rust;
          cargo = rust.cargo;
        });
        beancount-gocardless-importer = rustPlatform.buildRustPackage {
          name = manifest.name;
          version = manifest.version;
          cargoLock = {
            lockFile = ./Cargo.lock;
            outputHashes = {
              "beanru-0.1.0" = "sha256-lfwjmThymh5mue7uuGfcEvaxw//jun5JlfdTJvVE9n0=";
              "gocardless-0.0.1" = "sha256-ytGrPXDDGbzJPxHvtjG+iSWTqw6KmSrfiNiCubIYjp0=";
            };
          };
          src = pkgs.lib.cleanSource ./.;
          buildInputs = [
            pkgs.openssl.dev
          ];
          nativeBuildInputs = [
            pkgs.pkg-config
          ];
        };
      in
      rec
      {
        formatter = pkgs.nixpkgs-fmt;

        packages = flake-utils.lib.flattenTree {
          beancount-gocardless-importer = beancount-gocardless-importer;
        };
        defaultPackage = packages.beancount-gocardless-importer;

        devShells.default = pkgs.mkShell {
          nativeBuildInputs = beancount-gocardless-importer.nativeBuildInputs;
          buildInputs = beancount-gocardless-importer.buildInputs ++ [
            pkgs.bashInteractive
            pkgs.rust-analyzer
            rust.default
          ];
        };
      }
    );
}
