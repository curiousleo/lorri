{ pkgs, LORRI_ROOT, BUILD_REV_COUNT, RUN_TIME_CLOSURE, rust}:
let

  lorriBinDir = "${LORRI_ROOT}/target/debug";

  inherit (import ./execline.nix { inherit pkgs; })
    writeExecline;

  inherit (import ./lib.nix { inherit pkgs writeExecline; })
    pipe allCommandsSucceed
    pathAdd pathPrependBins;

  inherit (import ./sandbox.nix { inherit pkgs LORRI_ROOT writeExecline; })
    runInEmptyEnv;

  # shellcheck a file
  shellcheck = file: writeExecline "lint-shellcheck" {} [
    "cd" LORRI_ROOT
    "foreground" [ "${pkgs.coreutils}/bin/echo" "shellchecking ${file}" ]
    "${pkgs.shellcheck}/bin/shellcheck" "--shell" "bash" file
  ];

  stdenvDrvEnvdir = export: drvAttrs: pkgs.runCommand "dumped-env" drvAttrs ''
      mkdir $out
      unset HOME TMP TEMP TEMPDIR TMPDIR NIX_ENFORCE_PURITY
      ${pkgs.s6-portable-utils}/bin/s6-dumpenv $out
    '';

  cargoEnvironment =
    let
      # we have to use a few things from /usr/bin on Darwin
      unsandboxedTools = [
        # for GCC’s collect2
        "nm" "strip"
        # rust packages
        "dsymutil"
      ];
      darwinUnsandboxedBinutils = pipe unsandboxedTools [
        (map (tool: ''ln -sT "/usr/bin/${tool}" "$out/bin/${tool}"''))
        (lns: [''mkdir -p $out/bin''] ++ lns)
        (pkgs.lib.concatStringsSep "\n")
        (pkgs.runCommand "unsanboxed-binutils" {})
      ];
      darwinFrameworks = [
        pkgs.darwin.Security
        pkgs.darwin.apple_sdk.frameworks.CoreServices
        pkgs.darwin.apple_sdk.frameworks.CoreFoundation
      ];
    in
      # first TODO
      [ "${pkgs.s6}/bin/s6-envdir" "-fn"
           (stdenvDrvEnvdir [ "__impureHostDeps" "buildInputs" "NIX_IGNORE_LD_THROUGH_GCC" "NIX_COREFOUNDATION_RPATH" "MACOSX_DEPLOYMENT_TARGET" "CMAKE_OSX_ARCHITECTURES" "NIX_DONT_SET_RPATH" "PATH" ] { buildInputs = [ pkgs.darwin.Security ]; }) ]
      # we have to add the bin to PATH,
      # otherwise cargo doesn’t find its subcommands
      ++ (pathPrependBins
        ([ rust pkgs.stdenv.cc ]
        # cargo needs `nm` on Darwin for linking
        ++ pkgs.lib.optional pkgs.stdenv.isDarwin darwinUnsandboxedBinutils))
      ++ [
        "export" "NIX_LDFLAGS" "-F${pkgs.darwin.apple_sdk.frameworks.CoreFoundation}/Library/Frameworks -framework CoreFoundation -F${pkgs.darwin.apple_sdk.frameworks.Security}/Library/Frameworks -framework Security"
        "export" "BUILD_REV_COUNT" (toString BUILD_REV_COUNT)
        "export" "RUN_TIME_CLOSURE" RUN_TIME_CLOSURE
        "if" [ "${pkgs.coreutils}/bin/env" ]
      ];

  cargo = name: setup: args:
    writeExecline name {} (cargoEnvironment ++ setup ++ [ "cargo" ] ++ args);

  # the CI tests we want to run
  # Tests should not depend on each other (or block if they do),
  # so that they can run in parallel.
  # If a test changes files in the repository, sandbox it.
  tests = {

    shellcheck =
      let files = [
        "nix/bogus-nixpkgs/builder.sh"
        "src/ops/direnv/envrc.bash"
      ];
      in {
        description = "shellcheck ${pkgs.lib.concatStringsSep " and " files}";
        test = allCommandsSucceed "lint-shellcheck-all" (map shellcheck files);
      };

    cargo-fmt = {
      description = "cargo fmt was done";
      test = cargo "lint-cargo-fmt" [] [ "fmt" "--" "--check" ];
    };

    cargo-test = {
      description = "run cargo test";
      test = cargo "cargo-test"
        # the tests need bash and nix and direnv
        (pathPrependBins [ pkgs.coreutils pkgs.bash pkgs.nix pkgs.direnv ])
        [ "test" ];
    };

    cargo-clippy = {
      description = "run cargo clippy";
      test = cargo "cargo-clippy" [ "export" "RUSTFLAGS" "-D warnings" ] [ "clippy" ];
    };

    # TODO: it would be good to sandbox this (it changes files in the tree)
    # but somehow carnix needs to compile the whole friggin binary in order
    # to generate a few measly nix files …
    carnix = {
      description = "check carnix up-to-date";
      test = writeExecline "lint-carnix" {}
        (cargoEnvironment
        ++ pathPrependBins [
             pkgs.carnix
             # TODO: nix-prefetch-* should be patched into carnix
             pkgs.nix-prefetch-scripts
             # nix-prefetch-url, which itself requires tar and gzip
             pkgs.nix pkgs.gnutar pkgs.gzip
           ]
        ++ [
          "if" [ pkgs.runtimeShell "${LORRI_ROOT}/nix/update-carnix.sh" ]
          "${pkgs.gitMinimal}/bin/git" "diff" "--exit-code"
        ]);
    };

  };

  # clean the environment;
  # this is the only way we can have a non-diverging
  # environment between developer machine and CI
  emptyTestEnv = test:
    writeExecline "${test.name}-empty-env" {}
      [ (runInEmptyEnv [ "USER" "HOME" "TERM" ]) test ];

  testsWithEmptyEnv = pkgs.lib.mapAttrs
    (_: test: test // { test = emptyTestEnv test.test; }) tests;

  # Write a attrset which looks like
  # { "test description" = test-script-derviation }
  # to a script which can be read by `bats` (a simple testing framework).
  batsScript =
    let
      # add a few things to bats’ path that should really be patched upstream instead
      # TODO: upstream
      bats = writeExecline "bats" {}
        (pathPrependBins [ pkgs.coreutils pkgs.gnugrep ]
        ++ [ "${pkgs.bats}/bin/bats" "$@" ]);
      # see https://github.com/bats-core/bats-core/blob/f3a08d5d004d34afb2df4d79f923d241b8c9c462/README.md#file-descriptor-3-read-this-if-bats-hangs
      closeFD3 = "3>&-";
    in name: tests: pipe testsWithEmptyEnv [
      (pkgs.lib.mapAttrsToList
        # a bats test looks like:
        # @test "name of test" {
        #   … test code …
        # }
        # bats is very picky about the {} block (and the newlines).
        (_: test: "@test ${pkgs.lib.escapeShellArg test.description} {\n${test.test} ${closeFD3}\n}"))
      (pkgs.lib.concatStringsSep "\n")
      (pkgs.writeText "testsuite")
      (test-suite: writeExecline name {} [
        bats test-suite
      ])
    ];

  testsuite = batsScript "run-testsuite" tests;

in {
  inherit testsuite;
  # we want the single test attributes to have their environment emptied as well.
  tests = testsWithEmptyEnv;
}
