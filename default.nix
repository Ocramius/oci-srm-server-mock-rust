# https://github.com/Hoverbear/hoverbear.org/blob/417d50e1050cf201c88e74681074803a86ccd018/content/blog/2021-06-25-a-flake-for-your-crate/index.md#defaultnix
{ lib
, naersk
, stdenv
, clangStdenv
, hostPlatform
, targetPlatform
, pkg-config
, libiconv
, rustfmt
, cargo
, rustc
  # , llvmPackages # Optional
  # , protobuf     # Optional
}:

let
  cargoToml = (builtins.fromTOML (builtins.readFile ./Cargo.toml));
in

naersk.lib."${targetPlatform.system}".buildPackage rec {
  src = ./.;

  buildInputs = [
    rustfmt
    pkg-config
    cargo
    rustc
    libiconv
  ];
  checkInputs = [ cargo rustc ];

  doCheck = true;
  CARGO_BUILD_INCREMENTAL = "false";
  # static compilation requires more work: https://github.com/nix-community/naersk/blob/78789c30d64dea2396c9da516bbcc8db3a475207/examples/static-musl/flake.nix#L1-L49
  #CARGO_BUILD_TARGET = "x86_64-unknown-linux-musl";
  #CARGO_BUILD_RUSTFLAGS ="-C target-feature=+crt-static";
  RUST_BACKTRACE = "full";
  copyLibs = true;

  # Optional things you might need:
  #
  # If you depend on `libclang`:
  # LIBCLANG_PATH = "${llvmPackages.libclang}/lib";
  #
  # If you depend on protobuf:
  # PROTOC = "${protobuf}/bin/protoc";
  # PROTOC_INCLUDE = "${protobuf}/include";

  name = cargoToml.package.name;
  version = cargoToml.package.version;

  meta = with lib; {
    description = cargoToml.package.description;
    homepage = cargoToml.package.homepage;
    license = with licenses; [ mit ];
    maintainers = with maintainers; [ ];
  };
}
