# https://github.com/Hoverbear/hoverbear.org/blob/417d50e1050cf201c88e74681074803a86ccd018/content/blog/2021-06-25-a-flake-for-your-crate/index.md#flakenix
{
  description = "My cute Rust crate!";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    naersk.url = "github:nmattia/naersk";
    naersk.inputs.nixpkgs.follows = "nixpkgs";
  };

  outputs = { self, nixpkgs, naersk }:
    let
      cargoToml = (builtins.fromTOML (builtins.readFile ./Cargo.toml));
      supportedSystems = [ "x86_64-linux" "aarch64-linux" "x86_64-darwin" ];
      forAllSystems = f:
        nixpkgs.lib.genAttrs supportedSystems (system: f system);
    in
    {
      overlay = final: prev: {
        "${cargoToml.package.name}" = final.callPackage ./. { inherit naersk; };
      };

      packages = forAllSystems (system:
        let
          pkgs = import nixpkgs {
            inherit system;
            overlays = [ self.overlay ];
          };
        in
        {
          "${cargoToml.package.name}" = pkgs."${cargoToml.package.name}";
          haha = pkgs.dockerTools.buildImage {
            name = "oci-srm-server-mock-rust";
            config = {
              Cmd =
                [ "${pkgs.${cargoToml.package.name}}/bin/oci-srm-server-mock" ];
              Env = [
                "OCI_SRM_SERVER_MOCK_PORT=80"
                "OCI_SRM_SERVER_MOCK_BASE_URL=http://oci-srm-server-mock/"
                "PUNCHOUT_SERVER_LOGIN_URI=http://punchout-server/punch-in?foo=bar&pass=example-supersecret"
                "PUNCHOUT_SERVER_CONFIRMATION_URI=http://punchout-server/cxml-order-request-endpoint"
              ];
              ExposedPorts = { "80/tcp" = { }; };
            };
          };
        });

      defaultPackage = forAllSystems (system:
        (import nixpkgs {
          inherit system;
          overlays = [ self.overlay ];
        })."${cargoToml.package.name}");

      checks = forAllSystems (system:
        let
          pkgs = import nixpkgs {
            inherit system;
            overlays = [ self.overlay ];
          };
        in
        {
          format = pkgs.runCommand "check-format"
            {
              buildInputs = with pkgs; [ rustfmt cargo ];
            } ''
            #${pkgs.rustfmt}/bin/cargo-fmt fmt --manifest-path ${./.}/Cargo.toml -- --check
            ${pkgs.nixpkgs-fmt}/bin/nixpkgs-fmt --check ${./.}
            touch $out # it worked!
          '';
          "${cargoToml.package.name}" = pkgs."${cargoToml.package.name}";
        });
      devShell = forAllSystems (system:
        let
          pkgs = import nixpkgs {
            inherit system;
            overlays = [ self.overlay ];
          };
        in
        pkgs.mkShell {
          inputsFrom = with pkgs; [ pkgs."${cargoToml.package.name}" ];
          buildInputs = with pkgs; [ rustfmt nixpkgs-fmt ];
          # @TODO Unnecessary?
          #LIBCLANG_PATH = "${pkgs.llvmPackages.libclang.lib}/lib";
        });
    };
}
