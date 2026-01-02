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
        
        # Use stable Rust toolchain
        rustToolchain = pkgs.rust-bin.stable.latest.default.override {
          extensions = [ "rust-src" "rust-analyzer" ];
        };

        # Build dependencies
        nativeBuildInputs = with pkgs; [
          rustToolchain
          pkg-config
        ];

        buildInputs = with pkgs; [
          openssl
        ] ++ lib.optionals stdenv.isDarwin [
          darwin.apple_sdk.frameworks.Security
          darwin.apple_sdk.frameworks.SystemConfiguration
        ];

        # Common environment for development
        commonEnv = {
          RUST_BACKTRACE = "1";
        };

      in
      {
        # Package definition
        packages.default = pkgs.rustPlatform.buildRustPackage {
          pname = "urx";
          version = "0.8.0";

          src = ./.;

          cargoLock = {
            lockFile = ./Cargo.lock;
          };

          inherit nativeBuildInputs buildInputs;

          meta = with pkgs.lib; {
            description = "Extracts URLs from OSINT Archives for Security Insights";
            homepage = "https://github.com/hahwul/urx";
            license = licenses.mit;
            maintainers = [ ];
            mainProgram = "urx";
          };
        };

        # Development shell
        devShells.default = pkgs.mkShell (commonEnv // {
          inherit buildInputs;
          nativeBuildInputs = nativeBuildInputs ++ (with pkgs; [
            # Additional development tools
            cargo-watch
            cargo-edit
            just
          ]);

          shellHook = ''
            echo "🦀 URX development environment"
            echo "Run 'just' to see available commands"
            echo "Run 'cargo build' to build the project"
            echo "Run 'cargo test' to run tests"
          '';
        });

        # App for easy running
        apps.default = {
          type = "app";
          program = "${self.packages.${system}.default}/bin/urx";
        };
      }
    );
}