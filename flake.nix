{
  description = "URX - Extracts URLs from OSINT Archives for Security Insights";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { self, nixpkgs, flake-utils, rust-overlay }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs {
          inherit system overlays;
        };
        
        # Parse Cargo.toml to get version and metadata
        cargoToml = builtins.fromTOML (builtins.readFile ./Cargo.toml);
        pname = cargoToml.package.name;
        version = cargoToml.package.version;

        # Common build inputs needed for the project
        nativeBuildInputs = with pkgs; [
          pkg-config
          rustPlatform.bindgenHook
        ];

        buildInputs = with pkgs; [
          openssl
        ] ++ lib.optionals stdenv.isDarwin [
          darwin.apple_sdk.frameworks.Security
          darwin.apple_sdk.frameworks.SystemConfiguration
        ];

        # Rust toolchain for development shell
        rustToolchain = pkgs.rust-bin.stable.latest.default.override {
          extensions = [ "rust-src" "rust-analyzer" ];
        };

      in
      {
        packages = {
          default = pkgs.rustPlatform.buildRustPackage {
            inherit pname version;
            
            src = ./.;

            cargoLock = {
              lockFile = ./Cargo.lock;
            };

            inherit nativeBuildInputs buildInputs;

            # Skip tests during build as some require network access
            doCheck = false;

            meta = with pkgs.lib; {
              description = cargoToml.package.description;
              homepage = "https://github.com/hahwul/urx";
              license = licenses.mit;
              maintainers = [ ];
              mainProgram = "urx";
            };
          };
        };

        # Development shell with Rust toolchain and dependencies
        devShells.default = pkgs.mkShell {
          inherit buildInputs;
          
          nativeBuildInputs = nativeBuildInputs ++ (with pkgs; [
            # Rust toolchain with rust-src and rust-analyzer
            rustToolchain
            
            # Additional tools
            just
          ]);

          # Environment variables
          RUST_SRC_PATH = "${rustToolchain}/lib/rustlib/src/rust/library";

          shellHook = ''
            echo "🦀 URX Development Environment"
            echo "Rust version: $(rustc --version)"
            echo "Cargo version: $(cargo --version)"
            echo ""
            echo "Available commands:"
            echo "  cargo build          - Build the project"
            echo "  cargo test           - Run tests"
            echo "  cargo run            - Run URX"
            echo "  just                 - See available just recipes"
          '';
        };

        # Allow running: nix run github:hahwul/urx
        apps.default = {
          type = "app";
          program = "${self.packages.${system}.default}/bin/urx";
        };
      }
    );
}
