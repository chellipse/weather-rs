let
  rust_overlay = import (builtins.fetchTarball "https://github.com/oxalica/rust-overlay/archive/master.tar.gz");

  pkgs = import <nixos> { overlays = [ rust_overlay ]; };
  unstable = import <nixpkgs> { overlays = [ rust_overlay ]; };

  # rust = unstable.rust-bin.stable.latest.default.override {
      # extensions = [ "rust-src" ];
  # };

  # rust = unstable.rust-bin.beta.latest.default.override {
    # extensions = [ "rust-src" "rust-analyzer" ];
  # };

  rust = unstable.rust-bin.selectLatestNightlyWith (toolchain: toolchain.default.override {
    extensions = [ "rust-src" "rustc-codegen-cranelift-preview" ];
  });

in
  pkgs.mkShell {
    nativeBuildInputs = with unstable; [
    # nixd
    # gcc
    rust
    # rust-analyzer
    # dep
    openssl
    pkg-config
  ];

  # Certain Rust tools won't work without this
  # This can also be fixed by using oxalica/rust-overlay and specifying the rust-src extension
  # See https://discourse.nixos.org/t/rust-src-not-found-and-other-misadventures-of-developing-rust-on-nixos/11570/3?u=samuela. for more details.
  RUST_SRC_PATH = "${pkgs.rust.packages.stable.rustPlatform.rustLibSrc}";
}
