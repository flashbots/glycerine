{
  cfg,
  nixpkgs,
}: let
  system = cfg.system;
  pkgs = nixpkgs.legacyPackages."${system}";

  src = pkgs.fetchFromGitHub {
    owner = "ochinchina";
    repo = "supervisord";
    rev = "d5a5470d11af81b48ce5d38ec6e7c1a40663ad45"; # main @ 2026-06-10
    sha256 = "sha256-6pMhLFehP9VVIBGAo0kHCXNZhmNWt5Sn5vEoLLXJR18=";
  };
in
  pkgs.buildGoModule {
    src = src;
    name = "supervisord";
    vendorHash = "sha256-HI2WX/O7riU9g5Iimu0MMV9MQO/KF7VckbDbhsVjDBo=";
    ldflags = ["-s" "-w"];
    trimpath = true;
    buildMode = "pie";
    tags = ["netgo" "osusergo"];
    subPackages = ["."];
  }
