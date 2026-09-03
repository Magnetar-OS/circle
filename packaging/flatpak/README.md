# Flatpak

The manifest builds offline, which means the crate sources have to be
generated from the lockfile first:

```sh
# once
curl -O https://raw.githubusercontent.com/flatpak/flatpak-builder-tools/master/cargo/flatpak-cargo-generator.py

# whenever Cargo.lock changes
python3 flatpak-cargo-generator.py ../../Cargo.lock -o cargo-sources.json

flatpak-builder --user --install --force-clean build io.github.entro314labs.Circle.yml
```

`cargo-sources.json` is generated, not committed: it is a few megabytes of
checksums that would dominate every diff, and it is reproducible from
`Cargo.lock` in one command.

**The substrate is a git source, not a path.** `Cargo.toml` resolves
`cosmic-pim` at `../cosmic-pim`, so the manifest checks it out into exactly
that place relative to the build directory. When cosmic-pim starts publishing
tags, both this and `Cargo.toml` change together.
