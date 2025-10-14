{
  description = "oci-srm-server-mock, mocks interactions for OCI PunchOut/PunchIn and Call-Up interactions";

  inputs = {
    flake-utils.url = "github:numtide/flake-utils";
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    fenix = {
      url = "github:nix-community/fenix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    naersk = {
      url = "github:nmattia/naersk";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { self, fenix, flake-utils, nixpkgs, naersk, rust-overlay, ... }:
    flake-utils.lib.eachDefaultSystem (
      system: let
        pkgs = (import nixpkgs) {
          inherit system;

          # to allow for rust-rover to be installed
          config.allowUnfree = true;

          overlays = [
            (import rust-overlay)
          ];
        };

        toolchain = with fenix.packages.${system};
          combine [
            minimal.rustc
            minimal.cargo
            targets.x86_64-unknown-linux-musl.latest.rust-std
          ];

        rustToolchain = pkgs.rust-bin.stable.latest.default.override {
          extensions = [
            "rust-src"
            "clippy"
            "rustfmt"
          ];

          targets = [ "x86_64-unknown-linux-musl" ];
        };

        naersk' = naersk.lib.${system}.override {
          cargo = rustToolchain;
          rustc = rustToolchain;
        };

        built = naersk'.buildPackage {
          src = ./.;
          doCheck = true;
          nativeBuildInputs = with pkgs; [ pkgsStatic.stdenv.cc ];

          # Tells Cargo that we're building for musl.
          # (https://doc.rust-lang.org/cargo/reference/config.html#buildtarget)
          CARGO_BUILD_TARGET = "x86_64-unknown-linux-musl";

          # Tells Cargo to enable static compilation.
          # (https://doc.rust-lang.org/cargo/reference/config.html#buildrustflags)
          #
          # Note that the resulting binary might still be considered dynamically
          # linked by ldd, but that's just because the binary might have
          # position-independent-execution enabled.
          # (see: https://github.com/rust-lang/rust/issues/79624#issuecomment-737415388)
          CARGO_BUILD_RUSTFLAGS = "-C target-feature=+crt-static";
        };
      in {
        packages = {
          defaultPackage = built;

          docker-image = pkgs.dockerTools.buildLayeredImage {
            name = "oci-srm-server-mock-rust";
            config = {
              Cmd =
                [ "${built}/bin/oci-srm-server-mock" ];
              Env = [
                "OCI_SRM_SERVER_MOCK_PORT=80"
                "OCI_SRM_SERVER_MOCK_BASE_URL=http://oci-srm-server-mock/"
                "PUNCHOUT_SERVER_LOGIN_URI=http://punchout-server/punch-in?foo=bar&pass=example-supersecret"
                "PUNCHOUT_SERVER_CONFIRMATION_URI=http://punchout-server/cxml-order-request-endpoint"
              ];
              ExposedPorts = { "80/tcp" = { }; };
            };
          };
        };

        devShells = {
          default = pkgs.mkShell {
            name = "oci-srm-server-mock-rust dev shell";

            nativeBuildInputs = [
              # needed for Linux compilation overall
              pkgs.openssl
              pkgs.pkg-config

              pkgs.rustc
              pkgs.rust-analyzer
              pkgs.cargo
              pkgs.jetbrains.rust-rover
              rustToolchain
            ];

            # this overwrites `~/.rust-rover/toolchain` each time
            # there should be a way to scope it to this directory instead
            shellHook = ''
              mkdir -p ~/.rust-rover/toolchain

              ln -sfn ${rustToolchain}/lib ~/.rust-rover/toolchain
              ln -sfn ${rustToolchain}/bin ~/.rust-rover/toolchain

              export RUST_SRC_PATH="$HOME/.rust-rover/toolchain/lib/rustlib/src/rust/library"
            '';
          };
        };
      }
    );
}
