#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-only
#
# What a checkout must be true of before anything is built in it.
#
# Run from a Circle checkout with the substrate beside it at `../cosmic-pim`.
# Both callers run this same file — the CI workflow and `just verify-head` —
# because the previous version of these checks existed twice, once inlined in
# each, which is the duplicated-invariant bug they were written to catch.
#
# Every check prints the values it compared rather than a verdict. A check
# that prints only PASS is one you have to trust; a broken checker is legible
# only when it shows its work, and three of these were caught that way.
set -euo pipefail

fail() { echo; echo "$@"; exit 1; }

# --- The toolchain pin and the declared minimum ------------------------------
#
# `rust-version` is a *minimum*, so cargo refuses a manifest asking for more
# than the pinned channel and builds in silence when it asks for less. Raising
# the toolchain and forgetting the manifest is the silent direction, and is
# what a routine bump produces — so it is the one that needs a gate. Measured
# in both directions rather than recalled.
channel=$(sed -n 's/^channel *= *"\(.*\)"/\1/p' rust-toolchain.toml)
manifest=$(sed -n 's/^rust-version *= *"\(.*\)"/\1/p' Cargo.toml)
[ -n "$channel" ] || fail "no channel in rust-toolchain.toml"
[ -n "$manifest" ] || fail "no rust-version in Cargo.toml"
echo "toolchain: rust-toolchain.toml $channel, Cargo.toml $manifest"
[ "$channel" = "$manifest" ] || fail \
"rust-toolchain.toml pins $channel; Cargo.toml declares $manifest.
Raise both together — cargo will not tell you when the manifest is the lower."

# --- Local crates the lockfile names -----------------------------------------
#
# `cargo --locked` catches a lockfile that is stale. It cannot catch one that
# is too full, and that is the direction that hurts: a lock naming a crate
# from a repository CI does not check out resolves perfectly on the machine
# that committed it and fails only where it matters.
#
# A lockfile entry with no `source` is built from a local path. CI checks out
# this repository and cosmic-pim and nothing else, so the allowed set is
# derived from what cosmic-pim actually contains rather than written down —
# it stays true when the substrate gains a crate, and fails when it gains a
# repository.
#
# This is the file that gets staged without being read, because a lockfile
# diff always looks like noise, which is why it wants a check and not a habit.
[ -d ../cosmic-pim ] || fail "../cosmic-pim is missing; the path dependencies cannot resolve"

allowed=$(
    printf 'circle\n'
    for crate in ../cosmic-pim/crates/*/Cargo.toml; do
        sed -n 's/^name = "\(.*\)"/\1/p' "$crate" | head -1
    done
)
local_pkgs=$(awk '
    /^\[\[package\]\]/ { name=""; src=0; next }
    /^name = /         { gsub(/"/,""); name=$3; next }
    /^source = /       { src=1; next }
    /^$/               { if (name != "" && src == 0) print name; name="" }
    END                { if (name != "" && src == 0) print name }
' Cargo.lock)

echo "lockfile: local crates $(echo $local_pkgs)"
stray=$(comm -23 <(echo "$local_pkgs" | sort -u) <(echo "$allowed" | sort -u))
[ -z "$stray" ] || fail \
"Cargo.lock names local crates that CI does not check out:
$(echo "$stray" | sed 's/^/  /')
CI clones this repository and cosmic-pim only, so these resolve here and
nowhere else. Either they do not belong in the graph, or the workflow has to
fetch them in the same change."

echo "preflight: this checkout is what it claims to be"
