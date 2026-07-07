{
  cfg,
  nixpkgs,
  awsNitroUtil,
  kernel,
  supervisord,
  glycerine,
}: let
  system = cfg.system;
  pkgs = nixpkgs.legacyPackages."${system}";
  lib = pkgs.lib;

  nitro = awsNitroUtil.lib."${system}";

  arch =
    cfg.eif_arch
    or (
      if system == "x86_64-linux"
      then "x86_64"
      else if system == "aarch64-linux"
      then "aarch64"
      else throw "Unsupported EIF system '${system}'"
    );

  rustTarget =
    cfg.rust_target
    or (
      if arch == "x86_64"
      then "x86_64-unknown-linux-musl"
      else if arch == "aarch64"
      then "aarch64-unknown-linux-musl"
      else throw "Unsupported EIF architecture '${arch}'"
    );

  kernelImage =
    if arch == "aarch64"
    then "Image"
    else if arch == "x86_64"
    then "bzImage"
    else throw "Unsupported EIF architecture '${arch}'";

  kernelBlobs = kernel (cfg // {target = "all";});

  cacertPackage = pkgs.cacert;
  ccPackage = pkgs.stdenv.cc;
  coreutilsPackage = pkgs.coreutils;
  curlPackage = pkgs.curl;
  dnsutilsPackage = pkgs.dnsutils;
  gitPackage = pkgs.git;
  glibcDevPackage = pkgs.glibc.dev;
  glibcPackage = pkgs.glibc;
  gnumakePackage = pkgs.gnumake;
  iperf3Package = pkgs.iperf3;
  iproute2Package = pkgs.iproute2;
  libmnlDevPackage = pkgs.libmnl.dev;
  libmnlPackage = pkgs.libmnl;
  libnftnlDevPackage = pkgs.libnftnl;
  libnftnlPackage = pkgs.libnftnl;
  nettoolsPackage = pkgs.nettools;
  nftablesPackage = pkgs.nftables;
  opensshPackage = pkgs.openssh;
  pkgConfigPackage = pkgs.pkg-config;
  supervisordPackage = supervisord cfg;
  tcpdumpPackage = pkgs.tcpdump;
  tcpflowPackage = pkgs.tcpflow;
  vimPackage = pkgs.vim;
  zlibPackage = pkgs.zlib;

  glibcDynamicLinker =
    if arch == "x86_64"
    then "ld-linux-x86-64.so.2"
    else if arch == "aarch64"
    then "ld-linux-aarch64.so.1"
    else throw "Unsupported glibc dynamic linker for EIF architecture '${arch}'";

  glibcDynamicLinkerDir =
    if arch == "x86_64"
    then "lib64"
    else "lib";

  glycerinePackage =
    glycerine
    (cfg
      // {
        rust_target = rustTarget;
        eif_arch = arch;
        static = cfg.static or true;
      });

  rootfs = pkgs.runCommand "glycerine-enclave-dev-rootfs" {} ''
    mkdir -p \
      $out/bin \
      $out/dev \
      $out/etc \
      $out/etc/ssh \
      $out/etc/ssl/certs \
      $out/lib \
      $out/lib64 \
      $out/proc \
      $out/root/.ssh \
      $out/run \
      $out/sys \
      $out/tmp \
      $out/usr/bin \
      $out/usr/include \
      $out/usr/lib \
      $out/usr/lib/pkgconfig \
      $out/usr/lib64 \
      $out/usr/sbin \
      $out/usr/src \
      $out/var \
      $out/var/empty \
      $out/var/log \

    install -m 0644 ${./resources/etc/group}                 $out/etc/group
    install -m 0644 ${./resources/etc/nsswitch.conf}         $out/etc/nsswitch.conf
    install -m 0644 ${./resources/etc/passwd}                $out/etc/passwd
    install -m 0644 ${./resources/etc/resolv.conf}           $out/etc/resolv.conf
    install -m 0644 ${./resources/etc/ssh/sshd_config}       $out/etc/ssh/sshd_config
    install -m 0644 ${./resources/etc/supervisord.conf}      $out/etc/supervisord.conf
    install -m 0640 ${./resources/root/.profile}             $out/root/.profile
    install -m 0644 ${./resources/root/.ssh/authorized_keys} $out/root/.ssh/authorized_keys
    install -m 0755 ${./resources/usr/bin/start-enclave.sh}  $out/usr/bin/start-enclave.sh
    install -m 0755 ${./resources/usr/bin/start-sshd.sh}     $out/usr/bin/start-sshd.sh

    install -m 0644 ${cacertPackage}/etc/ssl/certs/ca-bundle.crt $out/etc/ssl/certs/ca-bundle.crt

    for header in ${glibcDevPackage}/include/*; do
      ln -s "$header" "$out/usr/include/$(basename "$header")"
    done
    for header in ${libmnlDevPackage}/include/*; do
      ln -s "$header" "$out/usr/include/$(basename "$header")"
    done
    for header in ${libnftnlDevPackage}/include/*; do
      ln -s "$header" "$out/usr/include/$(basename "$header")"
    done

    for lib in ${glibcPackage}/lib/*; do
      ln -s "$lib" "$out/lib/$(basename "$lib")"
    done
    for lib in ${libmnlPackage}/lib/*; do
      ln -s "$lib" "$out/lib/$(basename "$lib")"
    done
    for lib in ${libnftnlPackage}/lib/*; do
      ln -s "$lib" "$out/lib/$(basename "$lib")"
    done
    for lib in ${zlibPackage}/lib/*; do
      ln -s "$lib" "$out/lib/$(basename "$lib")"
    done

    ln -s ${libmnlDevPackage}/lib/pkgconfig/libmnl.pc     $out/usr/lib/pkgconfig/libmnl.pc
    ln -s ${libnftnlDevPackage}/lib/pkgconfig/libnftnl.pc $out/usr/lib/pkgconfig/libnftnl.pc

    if [ ! -e "$out/${glibcDynamicLinkerDir}/${glibcDynamicLinker}" ]; then
      ln -s /lib/${glibcDynamicLinker} $out/${glibcDynamicLinkerDir}/${glibcDynamicLinker}
    fi
    ln -s /lib/${glibcDynamicLinker} $out/usr/lib/${glibcDynamicLinker}

    install -m 0755 ${pkgs.busybox}/bin/busybox $out/bin/busybox
    for cmd in \
      cat \
      chmod \
      cp \
      grep \
      head \
      ln \
      ls \
      mkdir \
      mktemp \
      mount \
      mv \
      rm \
      rmdir \
      sh \
      stat \
      touch \
      uname \
    ; do
      ln -s /bin/busybox "$out/bin/$cmd"
    done

    for tool in ${ccPackage}/bin/*; do
      install -m 0755 "$tool" "$out/usr/bin/$(basename "$tool")"
    done

    install -m 0755 ${curlPackage}/bin/curl               $out/usr/bin/curl
    install -m 0755 ${dnsutilsPackage}/bin/dig            $out/usr/bin/dig
    install -m 0755 ${gitPackage}/bin/git                 $out/usr/bin/git
    install -m 0755 ${glycerinePackage}/bin/glycerine     $out/usr/bin/glycerine
    install -m 0755 ${gnumakePackage}/bin/make            $out/usr/bin/make
    install -m 0755 ${iperf3Package}/bin/iperf3           $out/usr/bin/iperf3
    install -m 0755 ${iproute2Package}/bin/ip             $out/usr/bin/ip
    install -m 0755 ${nettoolsPackage}/bin/ifconfig       $out/usr/bin/ifconfig
    install -m 0755 ${nftablesPackage}/bin/nft            $out/usr/bin/nft
    install -m 0755 ${opensshPackage}/bin/ssh-keygen      $out/usr/bin/ssh-keygen
    install -m 0755 ${opensshPackage}/bin/sshd            $out/usr/sbin/sshd
    install -m 0755 ${pkgConfigPackage}/bin/pkg-config    $out/usr/bin/pkg-config
    install -m 0755 ${supervisordPackage}/bin/supervisord $out/usr/bin/supervisord
    install -m 0755 ${tcpdumpPackage}/bin/tcpdump         $out/usr/bin/tcpdump
    install -m 0755 ${tcpflowPackage}/bin/tcpflow         $out/usr/bin/tcpflow
    install -m 0755 ${vimPackage}/bin/vim                 $out/usr/bin/vim

    if [ -f ${opensshPackage}/etc/ssh/moduli ]; then
      install -m 0644 ${opensshPackage}/etc/ssh/moduli $out/etc/ssh/moduli
    fi

    ln -s ca-bundle.crt $out/etc/ssl/certs/ca-certificates.crt
    ln -s certs/ca-bundle.crt $out/etc/ssl/cert.pem

    chmod 0700 $out/root/.ssh
    chmod 0600 $out/root/.ssh/authorized_keys
    chmod 0755 $out/var/empty
  '';

  entrypoint = lib.concatStringsSep "\n" [
    "/usr/bin/start-enclave.sh"
    ""
  ];

  env = lib.concatStringsSep "\n" [
    "AR=/usr/bin/ar"
    "C_INCLUDE_PATH=/usr/include"
    "CC=/usr/bin/cc"
    "CPLUS_INCLUDE_PATH=/usr/include"
    "CURL_CA_BUNDLE=/etc/ssl/certs/ca-bundle.crt"
    "CXX=/usr/bin/c++"
    "GIT_SSL_CAINFO=/etc/ssl/certs/ca-bundle.crt"
    "LD_LIBRARY_PATH=/lib:/lib64:/usr/lib:/usr/lib64"
    "LIBRARY_PATH=/lib:/lib64:/usr/lib:/usr/lib64"
    "PATH=/usr/sbin:/usr/bin:/sbin:/bin"
    "PKG_CONFIG_PATH=/usr/lib/pkgconfig"
    "PKG_CONFIG=/usr/bin/pkg-config"
    "RANLIB=/usr/bin/ranlib"
    "RUST_BACKTRACE=1"
    "SSL_CERT_FILE=/etc/ssl/certs/ca-bundle.crt"
    ""
  ];

  cmdline =
    cfg.cmdline
    or "reboot=k panic=30 pci=off nomodules console=ttyS0 i8042.noaux i8042.nomux i8042.nopnp i8042.dumbkbd random.trust_cpu=on";
in
  (nitro.buildEif {
    inherit arch cmdline entrypoint env;

    name = cfg.name or "glycerine-dev";
    version = cfg.version or "0.1.0";

    kernel = "${kernelBlobs}/${arch}/${kernelImage}";
    kernelConfig = "${kernelBlobs}/${arch}/${kernelImage}.config";

    init = "${kernelBlobs}/${arch}/init";
    nsmKo = "${kernelBlobs}/${arch}/nsm.ko";

    copyToRoot = rootfs;
    copyToRootWithClosure = true;
  })
  // {
    passthru = {
      inherit
        cacertPackage
        ccPackage
        coreutilsPackage
        curlPackage
        dnsutilsPackage
        gitPackage
        glibcDevPackage
        glibcPackage
        glycerinePackage
        gnumakePackage
        iperf3Package
        iproute2Package
        kernelBlobs
        libmnlDevPackage
        libmnlPackage
        libnftnlDevPackage
        libnftnlPackage
        nettoolsPackage
        nftablesPackage
        opensshPackage
        pkgConfigPackage
        rootfs
        supervisordPackage
        tcpdumpPackage
        tcpflowPackage
        vimPackage
        zlibPackage
        ;
      awsNitroUtil = nitro;
    };
  }
