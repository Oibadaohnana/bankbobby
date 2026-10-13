{ pkgs ? import <nixpkgs> { } }:

let
  # What winit and wgpu dlopen at run time: the window system and the GPU
  # loader. Nothing here is linked, so `cargo build` works without it —
  # `cargo run` is what wants LD_LIBRARY_PATH.
  runtimeLibs = with pkgs; [
    vulkan-loader
    libxkbcommon
    wayland
    libx11
    libxcursor
    libxi
    libxrandr
  ];
in
pkgs.mkShell {
  name = "bankbobbery";
  packages = with pkgs; [
    rustc
    cargo
    rustfmt
    clippy
    pkg-config
    # The headless compositor `./hidden` opens a window on.
    weston
  ];
  buildInputs = runtimeLibs;
  LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath runtimeLibs;
}
