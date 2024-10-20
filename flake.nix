{
  description = "SSH benchmark tools";

  inputs = {
    flake-utils.url = "github:numtide/flake-utils";
    naersk.url = "github:nix-community/naersk";
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
  };

  outputs = { self, flake-utils, naersk, nixpkgs }:
    let
      pkgs = nixpkgs.legacyPackages.x86_64-linux;

      naersk' = pkgs.callPackage naersk { };

      server = naersk'.buildPackage {
        pname = "server"; # Cargo workspace
        src = ./.;
      };
      serverImage = pkgs.dockerTools.buildLayeredImage {
        name = "ssh-server";
        tag = "latest";
        created = "now";
        config = {
          entrypoint = [ "${server}/bin/server" ];
          Labels = {
            "org.opencontainers.image.authors" = "pbar1";
            "org.opencontainers.image.source" = "https://github.com/pbar1/ssh-benchmark";
          };
        };
      };
    in
    {
      packages.aarch64-darwin = {
        inherit server serverImage;
      };

      packages.x86_64-linux = {
        inherit server serverImage;
      };
    };
}
