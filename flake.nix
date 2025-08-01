{
  description = "SFTP sync tool";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, flake-utils,  ... }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs { inherit system; };
        pname = "rmote";
        version = "0.1.0"; 
      in rec {
        packages.default = pkgs.rustPlatform.buildRustPackage {
          inherit pname version;
          src = self;

          cargoLock.lockFile = ./Cargo.lock;

          nativeBuildInputs = [ pkgs.pkg-config ];
          buildInputs = [ pkgs.openssl pkgs.libssh2 pkgs.zlib ];

          LIBSSH2_SYS_USE_PKG_CONFIG = "1";
          OPENSSL_NO_VENDOR = "1";

          meta = with pkgs.lib; {
            description = "Sync a local dir to a remote host over SFTP with debounced fs-watching";
            license = licenses.mit;
            mainProgram = pname;
          };
        };

        apps.default = {
          type = "app";
          program = "${packages.default}/bin/${pname}";
        };

        devShells.default = pkgs.mkShell {
          nativeBuildInputs = [ pkgs.pkg-config ];
          buildInputs = [ pkgs.openssl pkgs.libssh2 pkgs.zlib pkgs.rustc pkgs.cargo ];
          LIBSSH2_SYS_USE_PKG_CONFIG = "1";
          OPENSSL_NO_VENDOR = "1";
        };

        checks.default = packages.default;
      });
}
