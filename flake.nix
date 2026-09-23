{
  description = "Nix flake for rv, a reproducible package manager for R, written in rust";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay.url = "github:oxalica/rust-overlay";
  };

  outputs = { self, nixpkgs, flake-utils, rust-overlay }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs {
          inherit system overlays;
        };

        rustToolchain = pkgs.rust-bin.stable.latest.default;

        rvPackage = pkgs.rustPlatform.buildRustPackage {
          pname = "rv";
          version = with builtins; (fromTOML (readFile ./Cargo.toml)).package.version;

          src = ./.;

          cargoLock = {
            lockFile = ./Cargo.lock;
          };

          buildFeatures = [ "cli" ];

          nativeBuildInputs = [ pkgs.pkg-config ];
          buildInputs = [ pkgs.zlib ];

          meta = with pkgs.lib; {
            description = "A reproducible, fast and declarative package manager for R";
            homepage = "https://github.com/A2-ai/rv";
            license = licenses.mit;
            maintainers = [ ];
            mainProgram = "rv";
          };
        };
      in {
        packages.default = rvPackage;

        devShells.default = pkgs.mkShell {
          buildInputs = [
            rustToolchain
            # add any tool useful for a dev shell
          ];
        };
      });
}
