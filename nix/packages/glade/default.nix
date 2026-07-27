{
  lib,
  inputs,
  pkgs,
  ...
}:
let
  rust = (import "${inputs.self}/nix/lib/rust-toolchain.nix") { inherit pkgs; };

  src = inputs.self;
  workspacePackage = (fromTOML (builtins.readFile "${src}/Cargo.toml")).workspace.package;
in
rust.rustPlatform.buildRustPackage {
  inherit (workspacePackage) version;
  inherit src;
  pname = "glade";

  nativeBuildInputs = [
    rust.rustPlatform.bindgenHook
  ];

  cargoLock.lockFile = "${src}/Cargo.lock";
  doCheck = true;
  useNextest = true;

  meta = with lib; {
    description = "A tool for inserting whitespace between crowded code blocks";
    license = licenses.mit;
    mainProgram = "glade";
  };
}
