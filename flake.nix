{
  description = "sacredvote-civic-news — Sacred.Vote civic news aggregator Rust sidecar (#154)";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    crane = {
      url = "github:ipetkov/crane";
    };
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { self, nixpkgs, flake-utils, crane, rust-overlay }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [ (import rust-overlay) ];
        };

        # Pin the Rust toolchain — match the edition + features the sidecar uses.
        # 1.78+ has the lazy_cell stabilization that some axum 0.7 deps assume.
        rustToolchain = pkgs.rust-bin.stable."1.83.0".default.override {
          extensions = [ "rust-src" ];
        };

        craneLib = (crane.mkLib pkgs).overrideToolchain rustToolchain;

        # Source filter: include only what cargo actually needs, so changes to
        # README / deploy/ / .github/ don't invalidate the build cache.
        src = craneLib.cleanCargoSource ./.;

        commonArgs = {
          inherit src;
          strictDeps = true;
          buildInputs = with pkgs; [
            openssl
            # rustls-tls means we DON'T need openssl, but feed-rs may pull in
            # transitive deps that compile against system libs. Keep openssl
            # in buildInputs as a safety net; pure-rust feature flags strip
            # the dynamic linkage.
          ];
          nativeBuildInputs = with pkgs; [
            pkg-config
          ];
          # No tests against the network — feed-rs parse tests run in-process,
          # reqwest tests are unit-level.
          doCheck = true;
          # Reproducible builds: strip path metadata, vendor everything.
          OPENSSL_NO_VENDOR = "1";
        };

        # Two-stage build: first compile + cache all deps, then compile the
        # binary against the cached deps. Saves 2-3 minutes on incremental
        # builds.
        cargoArtifacts = craneLib.buildDepsOnly commonArgs;

        sacredvote-civic-news = craneLib.buildPackage (commonArgs // {
          inherit cargoArtifacts;
          # Strip the binary for a smaller closure.
          # Crane respects the [profile.release] strip = true in Cargo.toml.
        });
      in
      {
        # `nix build` -> release binary at result/bin/sacredvote-civic-news
        packages.default = sacredvote-civic-news;
        packages.sacredvote-civic-news = sacredvote-civic-news;

        # `nix run` -> runs the sidecar with the current shell's env vars
        apps.default = flake-utils.lib.mkApp {
          drv = sacredvote-civic-news;
        };

        # `nix develop` -> shell with rust toolchain + cargo + helpers
        devShells.default = pkgs.mkShell {
          inputsFrom = [ sacredvote-civic-news ];
          packages = with pkgs; [
            rustToolchain
            cargo-watch
            cargo-edit
            rust-analyzer
          ];
          shellHook = ''
            echo "sacredvote-civic-news devshell — Rust ${rustToolchain.version}"
          '';
        };

        # CI helper: `nix flake check` runs this + the default build.
        checks = {
          inherit sacredvote-civic-news;

          # Clippy as a separate check so a clippy regression doesn't block
          # the build (but does block CI).
          sacredvote-civic-news-clippy = craneLib.cargoClippy (commonArgs // {
            inherit cargoArtifacts;
            cargoClippyExtraArgs = "--all-targets -- --deny warnings";
          });

          # Test as a separate check too (parallels cargo test).
          sacredvote-civic-news-test = craneLib.cargoTest (commonArgs // {
            inherit cargoArtifacts;
          });

          # Format check — fails CI if rustfmt would reformat anything.
          sacredvote-civic-news-fmt = craneLib.cargoFmt {
            inherit src;
          };
        };
      }
    ) // {
      # NixOS module — drop into a NixOS configuration to deploy the sidecar:
      #
      #   inputs.sacredvote-civic-news.url = "github:thepictishbeast/sacredvote-civic-news";
      #   imports = [ inputs.sacredvote-civic-news.nixosModules.default ];
      #   services.sacredvote-civic-news.enable = true;
      #
      # The module wires the binary into systemd with the same hardening
      # flags as deploy/systemd/sacredvote-civic-news.service.
      nixosModules.default = { config, lib, pkgs, ... }:
        let
          cfg = config.services.sacredvote-civic-news;
          pkg = self.packages.${pkgs.system}.default;
        in
        {
          options.services.sacredvote-civic-news = {
            enable = lib.mkEnableOption "Sacred.Vote civic news aggregator sidecar";

            bind = lib.mkOption {
              type = lib.types.str;
              default = "127.0.0.1:3005";
              description = "Bind address. Loopback-only by default; never expose publicly.";
            };

            sources = lib.mkOption {
              type = lib.types.listOf lib.types.str;
              default = [ ];
              description = "List of HTTPS RSS/Atom feed URLs to aggregate.";
            };

            ratingsToml = lib.mkOption {
              type = lib.types.nullOr lib.types.path;
              default = null;
              description = "Path to a TOML file mapping source URLs to (bias, factual) ratings.";
            };

            logLevel = lib.mkOption {
              type = lib.types.str;
              default = "info";
              description = "Rust tracing level: trace, debug, info, warn, error.";
            };
          };

          config = lib.mkIf cfg.enable {
            users.users.civicnews = {
              isSystemUser = true;
              group = "civicnews";
              description = "Sacred.Vote civic-news sidecar user";
            };
            users.groups.civicnews = { };

            systemd.services.sacredvote-civic-news = {
              description = "Sacred.Vote civic news aggregator sidecar (#154)";
              after = [ "network-online.target" ];
              wants = [ "network-online.target" ];
              wantedBy = [ "multi-user.target" ];

              environment = {
                CIVIC_NEWS_BIND = cfg.bind;
                CIVIC_NEWS_SOURCES = lib.concatStringsSep "," cfg.sources;
                RUST_LOG = cfg.logLevel;
              } // lib.optionalAttrs (cfg.ratingsToml != null) {
                CIVIC_NEWS_RATINGS_TOML = toString cfg.ratingsToml;
              };

              serviceConfig = {
                Type = "simple";
                ExecStart = "${pkg}/bin/sacredvote-civic-news";
                User = "civicnews";
                Group = "civicnews";
                Restart = "on-failure";
                RestartSec = "5s";

                # Hardening — mirror deploy/systemd/sacredvote-civic-news.service.
                NoNewPrivileges = true;
                ProtectSystem = "strict";
                ProtectHome = true;
                PrivateTmp = true;
                PrivateDevices = true;
                ProtectKernelTunables = true;
                ProtectKernelModules = true;
                ProtectKernelLogs = true;
                ProtectClock = true;
                ProtectControlGroups = true;
                ProtectHostname = true;
                ProtectProc = "invisible";
                RestrictNamespaces = true;
                RestrictRealtime = true;
                RestrictSUIDSGID = true;
                LockPersonality = true;
                MemoryDenyWriteExecute = true;
                SystemCallArchitectures = "native";
                SystemCallFilter = [
                  "@system-service"
                  "~@mount @debug @cpu-emulation @obsolete @swap @raw-io @reboot @resources"
                ];
                CapabilityBoundingSet = "";
                AmbientCapabilities = "";

                RestrictAddressFamilies = [ "AF_INET" "AF_INET6" "AF_UNIX" ];

                MemoryMax = "512M";
                TasksMax = 64;
                CPUQuota = "50%";
                LimitNOFILE = 4096;
              };
            };
          };
        };
    };
}
