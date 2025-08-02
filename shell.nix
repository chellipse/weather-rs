let
  rust_overlay = import (
    builtins.fetchTarball {
      url = "https://github.com/oxalica/rust-overlay/archive/0e624f2b1972a34be1a9b35290ed18ea4b419b6f.tar.gz"; # master
      sha256 = "1z8i8gs2cfwxplr40dlwhgrc7d7wbx0ic7w8dcnfm936228p11rp"; # 2025-05-16T13·49+00
    }
  );

  pkgs = import (fetchTarball {
    url = "https://github.com/NixOS/nixpkgs/archive/adaa24fbf46737f3f1b5497bf64bae750f82942e.tar.gz"; # nixos-unstable
    sha256 = "0mmcni35fxs87fnhavfprspczgnnkxyizy8a4x57y98y76c4q4da"; # 2025-05-16T13·49+00
  }) { overlays = [ rust_overlay ]; };

  rust = pkgs.rust-bin.stable.latest.default.override {
    extensions = [ "rust-src" ];
  };
in
pkgs.mkShell {
  nativeBuildInputs = [
    rust

    ### dep ###
    pkgs.pkg-config
    pkgs.openssl
  ];

  RUST_SRC_PATH = "${pkgs.rust.packages.stable.rustPlatform.rustLibSrc}";
}
