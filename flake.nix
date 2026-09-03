{
  description = "Circle — contacts for the COSMIC desktop";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { nixpkgs, flake-utils, ... }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs { inherit system; };

        # libcosmic is a git dependency with no crates.io release, so the
        # lockfile is the reproducibility mechanism — see the conventions doc.
        # outputHashes has to name every git dependency the lock resolves.
        nativeBuildInputs = with pkgs; [
          cargo
          rustc
          just
          pkg-config
          desktop-file-utils
          appstream
        ];

        buildInputs = with pkgs; [
          openssl
          libxkbcommon
          wayland
          vulkan-loader
          # Passwords live in the OS keyring, never in accounts.toml.
          libsecret
        ];
      in
      {
        devShells.default = pkgs.mkShell {
          inherit nativeBuildInputs buildInputs;

          # wgpu resolves its backend at runtime; without this the binary built
          # in the shell finds no Vulkan ICD and falls back to software or
          # fails outright.
          LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath buildInputs;

          shellHook = ''
            echo "Circle dev shell. 'just run' to build and start, 'just check-all' before pushing."
            echo "A sibling checkout of cosmic-pim is required — see the README."
          '';
        };

        formatter = pkgs.nixpkgs-fmt;
      });
}
