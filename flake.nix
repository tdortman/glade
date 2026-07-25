{

  description = "A tool for inserting whitespace between crowded code blocks";

  outputs =
    inputs:
    inputs.snowfall-lib.mkFlake {
      inherit inputs;

      snowfall = {
        namespace = "glade";
        root = ./nix;
      };

      src = ./.;
      alias.packages.default = "glade";

      overlays = [
        inputs.rust-overlay.overlays.default
      ];

      supportedSystems = [
        "x86_64-linux"
        "aarch64-linux"
      ];

      outputs-builder =
        channels:
        let
          treefmt = inputs.treefmt-nix.lib.evalModule channels.nixpkgs {
            imports = [ inputs.pedantix.treefmtModules.default ];

            programs.pedantix = {
              enable = true;

              settings = {
                attrs = {
                  blank-lines = 1;
                  blank-lines-mode = "multiline";
                  flatten = true;
                  merge = true;
                };

                formatter = "nixfmt";
                inherit-placement = "front";
                lists.sort = false;
              };
            };

            projectRootFile = "flake.nix";
          };
        in
        {
          checks.formatting = treefmt.config.build.check inputs.self;
          formatter = treefmt.config.build.wrapper;
        };
    };

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

    pedantix = {
      inputs = {
        nixpkgs.follows = "nixpkgs";
        treefmt-nix.follows = "treefmt-nix";
      };

      url = "github:swarsel/pedantix";
    };

    rust-overlay = {
      inputs.nixpkgs.follows = "nixpkgs";
      url = "github:oxalica/rust-overlay";
    };

    snowfall-lib = {
      inputs.nixpkgs.follows = "nixpkgs";
      url = "github:anntnzrb/snowfall-lib";
    };

    treefmt-nix = {
      inputs.nixpkgs.follows = "nixpkgs";
      url = "github:numtide/treefmt-nix";
    };
  };
}
