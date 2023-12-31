{ pkgs ? import <nixpkgs> {} }:
pkgs.mkShell {
  nativeBuildInputs = with pkgs; [
    ### nix stuff
    nixd
    ### rust
    gcc
    rustc
    cargo
    rust-analyzer
    rustfmt
    clippy
    ### dep
    openssl
    pkg-config
  ];

  shellHook = ''
    echo "Gcc $(gcc --version | head -n 1 | awk '{print $3}'), Rustc $(rustc --version | awk '{print $2}'), "
  '';

  # Certain Rust tools won't work without this
  # This can also be fixed by using oxalica/rust-overlay and specifying the rust-src extension
  # See https://discourse.nixos.org/t/rust-src-not-found-and-other-misadventures-of-developing-rust-on-nixos/11570/3?u=samuela. for more details.
  RUST_SRC_PATH = "${pkgs.rust.packages.stable.rustPlatform.rustLibSrc}";
}
