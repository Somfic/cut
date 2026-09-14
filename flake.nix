{
  description = "cut";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs =
    {
      self,
      nixpkgs,
      rust-overlay,
      flake-utils,
    }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs { inherit system overlays; };

        rustToolchain = pkgs.rust-bin.stable.latest.default.override {
          extensions = [
            "rust-src"
            "rust-analyzer"
            "clippy"
          ];
        };

        linuxLibs = with pkgs; [
          vulkan-loader
          wayland
          libxkbcommon
          libGL
          xorg.libX11
          xorg.libXcursor
          xorg.libXrandr
          xorg.libXi
          # tauri's webview
          webkitgtk_4_1
          libsoup_3
        ];

        runtimeLibs =
          (with pkgs; [
            gst_all_1.gstreamer
            gst_all_1.gst-plugins-base
          ])
          ++ pkgs.lib.optionals pkgs.stdenv.isLinux linuxLibs;
      in
      {
        devShells.default = pkgs.mkShell {
          nativeBuildInputs = with pkgs; [
            pkg-config
          ];

          buildInputs =
            (with pkgs; [
              rustToolchain
              cargo-tauri

              # frontend
              bun
              nodejs

              # gstreamer stack
              glib
              gst_all_1.gstreamer
              gst_all_1.gst-plugins-base
              gst_all_1.gst-plugins-good
              gst_all_1.gst-plugins-bad
              gst_all_1.gst-plugins-ugly
              gst_all_1.gst-libav

              # tooling
              ripgrep
              fd
              bat
            ])
            ++ pkgs.lib.optionals pkgs.stdenv.isLinux (
              with pkgs;
              [
                # audio (cpal); CoreAudio covers this on darwin
                alsa-lib
                gtk3
                glib-networking
                librsvg
                openssl
              ]
              ++ linuxLibs
            );

          shellHook = ''
            export PATH="$HOME/.cargo/bin:$PATH"
            export LD_LIBRARY_PATH="${pkgs.lib.makeLibraryPath runtimeLibs}:$LD_LIBRARY_PATH"
          '';

          RUST_BACKTRACE = "1";
          RUST_LOG = "debug";
        };

        formatter = pkgs.nixpkgs-fmt;
      }
    );
}
