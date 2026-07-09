{
  cfg,
  nixpkgs,
}: let
  aws66 = import ./aws-6.6.nix {
    inherit nixpkgs;
    cfg = cfg // {target = "all";};
  };

  inherit (aws66.passthru) src awsBootstrapOutputsWithKernel awsBootstrapPkgs;

  linux = {
    version = "6.12.94";
    hash = "sha256-ZZECsuPcvZna5lIjBsUujaPNRABk79nx9SV9dE9r3P0=";
  };

  target = cfg.target or "all";
  select = outputs:
    if builtins.hasAttr target outputs
    then outputs.${target}
    else throw "aws-nitro-enclaves-sdk-bootstrap target '${target}' not found";

  kernel =
    (import (src + "/kernel/kernel.nix") {
      pkgs = awsBootstrapPkgs;
    }).overrideAttrs (original: {
      version = linux.version;

      src = awsBootstrapPkgs.fetchFromGitHub {
        owner = "gregkh";
        repo = "linux";
        rev = "v${linux.version}";
        hash = linux.hash;
      };

      # NSM is upstream in Linux 6.12; keep only the vsock fix needed here.
      patches = [
        ./patches/0001-vsock-virtio-remove-queued-replies-linux-6.12.patch
      ];

      files =
        (original.files or [])
        ++ [
          (awsBootstrapPkgs.writeText "microvm-kernel-config-netfilter" ''
            CONFIG_NETFILTER_NETLINK=y
            CONFIG_NETFILTER_NETLINK_QUEUE=y
            CONFIG_NF_TABLES=y
            CONFIG_NF_TABLES_IPV4=y
            CONFIG_NFT_QUEUE=y
          '')
        ];

      meta =
        (original.meta or {})
        // {
          description = "Linux Kernel ${linux.version} for Nitro Enclaves";
        };
    });

  aws612BootstrapOutputs = awsBootstrapOutputsWithKernel kernel;

  inherit (aws612BootstrapOutputs) init linuxkit;

  arch = awsBootstrapPkgs.stdenv.hostPlatform.uname.processor;
  all = aws612BootstrapOutputs.all.overrideAttrs {
    name = "enclaves-blobs-${arch}-linux-${kernel.version}";
  };

  outputs = {
    inherit all init kernel linuxkit;
  };
in
  select outputs
