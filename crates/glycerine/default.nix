{
  cfg,
  nixpkgs,
  fenix,
  naersk,
}: let
  manifest = fromTOML (builtins.readFile ./Cargo.toml);

  system = cfg.system;
  pkgs = nixpkgs.legacyPackages."${system}";

  target = cfg.rust_target;
  targetEnv = pkgs.lib.toUpper (builtins.replaceStrings ["-"] ["_"] target);
  targetPkgs = pkgs.pkgsCross.musl64;

  toolchain = with fenix.packages.${system};
    combine [
      stable.cargo
      stable.rustc
      targets.${target}.stable.rust-std
    ];

  naerskWithOverrides = naersk.lib.${system}.override {
    cargo = toolchain;
    rustc = toolchain;
  };

  cc =
    if cfg.static
    then targetPkgs.stdenv.cc
    else pkgs.stdenv.cc;

  staticLibmnl = targetPkgs.libmnl.overrideAttrs (old: {
    configureFlags = (old.configureFlags or []) ++ ["--enable-static"];
    dontDisableStatic = true;
  });
  staticLibnftnl = (targetPkgs.libnftnl.override {libmnl = staticLibmnl;}).overrideAttrs (old: {
    configureFlags = (old.configureFlags or []) ++ ["--enable-static"];
    dontDisableStatic = true;
  });

  targetBuildInputs =
    if cfg.static
    then [
      staticLibmnl
      staticLibnftnl
    ]
    else
      with pkgs; [
        libmnl
        libnftnl
      ];

  pkgConfigEnv =
    if cfg.static
    then {
      "PKG_CONFIG_ALLOW_CROSS" = "1";
      "PKG_CONFIG_ALL_STATIC" = "1";
    }
    else {};

  packageAttrs = rec {
    name = manifest.package.name;
    version = manifest.package.version;

    src = ../..;

    "CARGO_BUILD_TARGET" = target;
    "CARGO_TARGET_${targetEnv}_LINKER" = TARGET_CC;
    "TARGET_CC" = "${cc}/bin/${cc.targetPrefix}cc";

    nativeBuildInputs = [
      cc
      pkgs.pkg-config
    ];
    buildInputs = targetBuildInputs;
  };
in
  naerskWithOverrides.buildPackage (packageAttrs // pkgConfigEnv)
