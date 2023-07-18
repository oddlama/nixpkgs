{
lib,
stdenv,

# Build-time dependencies:
bash,
bazel_6,
buildBazelPackage,
cmake,
fetchFromGitHub,
git,
gnumake,
perl,
#ninja,

# Runtime dependencies:
#fetchurl,
zlib,
#openssl,
#snappy,
#icu,
#gnugrep,
#python3,
#which,

breakpointHook
}:
let
  system = stdenv.hostPlatform.system;
in
buildBazelPackage rec {
  pname = "typesense";
  version = "0.25.0";

  src = fetchFromGitHub {
    owner = "typesense";
    repo = "typesense";
    rev = "refs/heads/v0.25-join"; # TODO change once released
    hash = "sha256-C4Is5sxG3UwIsmzl6/K/4kJt590hh0TvdOrqIRePhYs=";
  };

  patches = [
    ./0001-add-bazel-shebang-patch.patch
    ./0002-use-existing-toolchains.patch
  ];

  postPatch = ''
    cp ${./foreign_cc_shebang.patch} bazel/foreign_cc_shebang.patch
    substituteInPlace bazel/foreign_cc_shebang.patch \
      --replace "@bash@" "${bash}/bin/bash"
    substituteInPlace bazel/libfor.BUILD \
      --replace "perl" "${perl}/bin/perl"
  '';

  bazel = bazel_6;

  dontUseCmakeConfigure = true;
  dontUseNinjaInstall = true;

  fetchAttrs = {
    sha256 = "sha256-/J1W/LB08lGs1cBzGBHytu7JpK1CA+2GPMZniABkxws=";
    nativeBuildInputs = [ git ];
  };

  buildAttrs = {
    nativeBuildInputs = [
      cmake
      zlib
      # Test if needed TODO
      #ninja
      gnumake
      perl
    ];

    preBuild = ''
      patchShebangs --build .
      # This is necessary since microsoft/onnxruntime has a path including unicode characters
      # which some part of the build system seems to gobble up in case the locale is not set
      # to C.UTF-8.
      export LANG="C.UTF-8"
    '';

    installPhase = ''
      install -Dm0755 bazel-bin/source/exe/envoy-static $out/bin/envoy
    '';
  };

  removeRulesCC = false;
  bazelTargets = [ "//:typesense-server" ];
  bazelBuildFlags = [
    "-c opt"
    #"--incompatible_strict_action_env"
    #"--action_env=BAZEL_CXXOPTS=-std=c++17"
    #"--define=TYPESENSE_VERSION=nightly"
    #"--cxxopt=-std=c++17"
    ## "--strip=never"
    ##"--sandbox_debug" # TODO away
    ##"--verbose_failures" # TODO away
    #"--define=BRPC_WITH_GLOG=true"
    #"--enable_platform_specific_config"
    #"--action_env=BAZEL_LINKLIBS=\"-l%:libstdc++.a -l%:libgcc.a\""
  ];

  meta = with lib; {
    homepage = "https://typesense.org";
    description = "Typesense is a modern, privacy-friendly, open source search engine built from the ground up using cutting-edge search algorithms, that take advantage of the latest advances in hardware capabilities.";
    license = licenses.gpl3;
    platforms = platforms.all;
    maintainers = with maintainers; [oddlama];
  };
}




#{
#  lib,
#  fetchFromGitHub,
#  fetchurl,
#  stdenv,
#  cmake,
#  pkg-config,
#  zlib,
#  openssl_1_1,
#  snappy,
#  icu,
#  perl,
#  gnumake,
#  gnugrep,
#  python3,
#  which,
#}: let
#  externalLibs = lib.mapAttrs (_: v:
#    v
#    // {
#      path = fetchurl {inherit (v) url hash;};
#    }) {
#    libfor = rec {
#      version = "49611808d08d4e47116aa2a3ddcabeb418f405f7";
#      expectedName = "libfor-${version}.tar.gz";
#      url = "https://github.com/cruppstahl/libfor/archive/${version}.tar.gz";
#      hash = "sha256-/CLmX2rTfAGXOiTpzaAeOv/h9oxwEq+agOaxtU1wEBg=";
#    };
#    h2o = rec {
#      version = "6dda7d6f21610ecd5256543384fa4b4b345a88ac";
#      expectedName = "h2o-${version}.tar.gz";
#      url = "https://github.com/h2o/h2o/archive/${version}.tar.gz";
#      hash = "sha256-2/p8LDjNkLwRtyZB+v72s0R74AbucDEIpE2S5oU3xUI=";
#    };
#    gtest = rec {
#      version = "1.8.0";
#      expectedName = "googletest-release-${version}.tar.gz";
#      url = "https://github.com/google/googletest/archive/release-${version}.tar.gz";
#      hash = "sha256-WKb0J3yivIVlIis7vVihd2CenEiOinJkk1m6UUUNt9g=";
#    };
#    iconv = rec {
#      version = "1.15";
#      expectedName = "libiconv-${version}.tar.gz";
#      url = "https://ftp.gnu.org/pub/gnu/libiconv/libiconv-${version}.tar.gz";
#      hash = "sha256-zPU2YgpFRY0muoOIepg7loJwAekqE4R7ReSSXMiRMXg=";
#    };
#    jemalloc = rec {
#      version = "5.3.0";
#      expectedName = "jemalloc-${version}.tar.bz2";
#      url = "https://github.com/jemalloc/jemalloc/releases/download/${version}/jemalloc-${version}.tar.bz2";
#      hash = "sha256-LbgtHnEZ3z5xt2QCGbbf6EeJvAU3mDw7esT3GJrs/qo=";
#    };
#    rocksdb = rec {
#      version = "6.20.3";
#      expectedName = "rocksdb-${version}.tar.gz";
#      url = "https://github.com/facebook/rocksdb/archive/v${version}.tar.gz";
#      hash = "sha256-xlAseq5kG34g+vpsK5InPZNdK3snBxNevZpnsJIWnco=";
#    };
#    hnswlib = rec {
#      version = "21de18ffabea1a9d1e8b16b49afc6045d7707e4c";
#      expectedName = "hnswlib-${version}.tar.gz";
#      url = "https://github.com/typesense/hnswlib/archive/${version}.tar.gz";
#      hash = "sha256-wcB8bQyQEZwfHUGmffFEs3kZZjkhFVENrlHhXlmLGfU=";
#    };
#    kakasi = rec {
#      version = "9e0825a02c7ea5605e968f6208f769f7c49d6860";
#      expectedName = "kakasi-${version}.tar.gz";
#      url = "https://github.com/typesense/kakasi/archive/${version}.tar.gz";
#      hash = "sha256-Tyg6Dx0fbSoAPkxloGqmdpjeai0wq+eC/SFLqp2n+dM=";
#    };
#    lru-cache = rec {
#      version = "13f30ad33a227a3e9682578c450777380ecddfcf";
#      expectedName = "lru-cache-${version}.tar.gz";
#      url = "https://github.com/goldsborough/lru-cache/archive/${version}.tar.gz";
#      hash = "sha256-Ngl9v0D1zoPrFGu2F28Q8LKbC5yZGweVzBNZ1RvmpQY=";
#    };
#    s2geometry = rec {
#      version = "efb124d8eaf3433323d3e877dedd5e94a63339a3";
#      expectedName = "s2geometry-${version}.tar.gz";
#      url = "https://github.com/google/s2geometry/archive/${version}.tar.gz";
#      hash = "sha256-VG2nU4c86WxgXowhaylVDDIoXyjFLMjDyxF1K2ALGL8=";
#    };
#  };
#in
#  stdenv.mkDerivation rec {
#    pname = "typesense";
#    version = "0.24.1";
#
#    src = fetchFromGitHub {
#      owner = "typesense";
#      repo = "typesense";
#      rev = "refs/tags/v${version}";
#      hash = "sha256-lowO/sDv//hcKytza4mAyeD9E5vP9WcgHQ/U0q12RWQ=";
#    };
#
#    nativeBuildInputs = [
#      cmake
#      pkg-config
#    ];
#
#    buildInputs = [
#      snappy
#      zlib
#      openssl_1_1
#      icu
#      perl
#      stdenv.cc.cc
#      gnumake
#      gnugrep
#      which
#      python3
#      #brotli
#      #libuv
#    ];
#
#    postUnpack = ''
#      mkdir -p source/external-deps
#    '' + lib.concatMapStrings
#      (v: ''
#        (
#          cd source/external-deps
#          ln -s ${v.path} ${v.expectedName}
#          tar xf ${v.expectedName}
#        )
#      '')
#      (lib.attrValues externalLibs)
#    + ''
#      patchShebangs --build source/external-deps
#    '';
#
#    patches = [
#      ./0001-fix-abs-edge-case.patch
#    ];
#
#    preConfigure = ''
#      # we don't want to deal with changing dependency paths based on the system we're on
#      substituteInPlace CMakeLists.txt \
#        --replace "external-\''${CMAKE_SYSTEM_NAME}" "external-deps"
#    '';
#
#    #buildPhase = ''
#    #  ./build.sh
#    #'';
#
#    cmakeFlags = [
#      "-DTYPESENSE_VERSION=nightly"
#    ];
#
#    doCheck = false;
#
#    meta = with lib; {
#      homepage = "https://typesense.org";
#      description = "Typesense is a modern, privacy-friendly, open source search engine built from the ground up using cutting-edge search algorithms, that take advantage of the latest advances in hardware capabilities.";
#      license = licenses.gpl3;
#      platforms = platforms.all;
#      maintainers = with lib.maintainers; [oddlama];
#    };
#  }
