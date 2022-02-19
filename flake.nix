{
  description = "transit_model";

  inputs.flake-utils.url = "github:numtide/flake-utils";

  outputs = { self, nixpkgs, flake-utils }:
    flake-utils.lib.eachDefaultSystem
      (system:
        # let pkgs = nixpkgs.legacyPackages.${system}; in
        let pkgs = import nixpkgs {
          inherit system;
          config = { allowUnfree = true; };
        }; in with pkgs;
        {
          devShell = mkShell {
              RUST_BACKTRACE = 1;
              LIBCLANG_PATH = "${libclang.lib}/lib";
              buildInputs = [
                rustc cargo
                pkgconfig
                clang proj
              ];

            };
        }
      );
}
