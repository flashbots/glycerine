{
  cfg,
  nixpkgs,
}: let
  system = cfg.system;
  pkgs = nixpkgs.legacyPackages."${system}";

  src = pkgs.fetchFromGitHub {
    owner = "aws";
    repo = "aws-nitro-enclaves-sdk-bootstrap";
    rev = "f718dea60a9d9bb8b8682fd852ad793912f3c5db"; # v6.6.79 / main @ 2025-02-24
    hash = "sha256-DmNnH6lqweRz1u8nza6FvgJFvDGPactpHruyq0SYkp8=";
  };

  target = cfg.target or "all";

  awsBootstrapPkgs = import (src + "/nixpkgs.nix") {
    inherit system;
  };

  awsBootstrapOutputs = import src {
    pkgs = awsBootstrapPkgs;
  };

  awsBootstrapOutputsWithKernel = kernel: let
    kernelPath = src + "/kernel/kernel.nix";
    pkgsWithKernel =
      awsBootstrapPkgs
      // {
        callPackage = path: args:
          if builtins.toString path == builtins.toString kernelPath
          then kernel
          else awsBootstrapPkgs.callPackage path args;
      };
  in
    import src {
      pkgs = pkgsWithKernel;
    };

  select = outputs:
    if builtins.hasAttr target outputs
    then outputs.${target}
    else throw "aws-nitro-enclaves-sdk-bootstrap target '${target}' not found";

  selectedAwsBootstrapOutput = select awsBootstrapOutputs;
in
  selectedAwsBootstrapOutput.overrideAttrs (original: {
    passthru =
      (original.passthru or {})
      // {
        inherit awsBootstrapOutputs awsBootstrapOutputsWithKernel awsBootstrapPkgs src system target;
        bootstrap = awsBootstrapOutputs;
        bootstrapPkgs = awsBootstrapPkgs;
      };
  })
