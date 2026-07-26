{ pkgs, inputs, ... }:
let
  rust = (import "${inputs.self}/nix/lib/rust-toolchain.nix") { inherit pkgs; };
in
pkgs.mkShell {
  nativeBuildInputs = with pkgs; [
    cargo-flamegraph
    cargo-nextest
    cmake
    pkg-config
    rust.rustPlatform.bindgenHook
    rust.toolchain
  ];
}
