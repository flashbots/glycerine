{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/release-26.05";

    # rust toolchains
    fenix = {
      url = "github:nix-community/fenix";
      inputs.nixpkgs.follows = "nixpkgs";
    };

    # rust packages builder
    naersk = {
      url = "github:nix-community/naersk";
      inputs.nixpkgs.follows = "nixpkgs";
    };

    # EIF image builder
    awsNitroUtil = {
      url = "github:monzo/aws-nitro-util";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = {
    self,
    nixpkgs,
    fenix, # rust toolchains
    naersk, # rust packages builder
    awsNitroUtil,
  }: let
    systems = [
      "x86_64-linux"
      "aarch64-darwin"
    ];

    forAllSystems = f: nixpkgs.lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});

    glycerine = cfg:
      import ./crates/glycerine {
        inherit cfg nixpkgs fenix naersk;
      };

    enclave.kernel.v6_6 = cfg:
      import ./enclave/_kernel/aws-6.6.nix {
        inherit cfg nixpkgs;
      };

    enclave.kernel.v6_12 = cfg:
      import ./enclave/_kernel/aws-6.12.nix {
        inherit cfg nixpkgs;
      };

    enclave.service.supervisord = cfg:
      import ./enclave/_supervisord {
        inherit cfg nixpkgs;
      };

    enclave.dev = cfg:
      import ./enclave/dev {
        inherit cfg nixpkgs awsNitroUtil;
        kernel = enclave.kernel.v6_12;
        supervisord = enclave.service.supervisord;
        glycerine = glycerine;
      };
  in {
    formatter = forAllSystems (pkgs: pkgs.alejandra);

    packages.x86_64-linux = {
      glycerine = glycerine {
        system = "x86_64-linux";
        rust_target = "x86_64-unknown-linux-musl";
        eif_arch = "x86_64";
        static = true;
      };

      enclave.kernel.v6_6 = enclave.kernel.v6_6 {
        system = "x86_64-linux";
      };

      enclave.kernel.v6_12 = enclave.kernel.v6_12 {
        system = "x86_64-linux";
      };

      enclave.supervisord = enclave.service.supervisord {
        system = "x86_64-linux";
      };

      enclave.dev = enclave.dev {
        system = "x86_64-linux";
      };
    };
  };
}
