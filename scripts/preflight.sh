#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-only
#
# What a checkout must be true of before anything is built in it.
#
# Run from a Circle checkout; it needs nothing beside it.
# Both callers run this same file — the CI workflow and `just verify-head` —
# because the previous version of these checks existed twice, once inlined in
# each, which is the duplicated-invariant bug they were written to catch.
#
# Every check prints the values it compared rather than a verdict. A check
# that prints only PASS is one you have to trust; a broken checker is legible
# only when it shows its work, and three of these were caught that way.
set -euo pipefail

fail() { echo; echo "$@"; exit 1; }

# --- Paths named by configuration --------------------------------------------
#
# Every literal repository path named in packaging, CI, or these scripts must
# exist. Nothing builds or opens most of them, so an absence is invisible
# until the job that names them runs.
#
# This runs FIRST, before any check reads a file. Deleting rust-toolchain.toml
# used to produce "sed: can't read rust-toolchain.toml" and exit 2, because
# the toolchain check read it before this one could say it was missing: the
# gate failed, but printed the least informative sentence available while
# holding the most informative one. A check that can explain a failure has to
# run before the checks that merely suffer from it.
#
# The pattern includes this script's own inputs on purpose. Matching only
# directory-prefixed paths left the sweep blind to `Cargo.toml`,
# `Cargo.lock` and `rust-toolchain.toml` — the three files the other checks
# read — which is the same narrowness as covering only the icons. `debian/circle` is excluded by
# name and with a reason — it is the staging root `just install` writes into,
# so it is generated rather than tracked. A *rule* for skipping absent paths
# is what this check must not have: "it does not exist, so it is probably
# generated" is an assertion in a comment, and comments asserting properties
# are the thing all of this was written to stop.
named=$(grep -ohE '((resources|i18n|scripts|debian|packaging)/[A-Za-z0-9._/-]+|Cargo\.(toml|lock)|rust-toolchain\.toml)' \
    justfile packaging/flatpak/*.yml debian/rules .github/workflows/*.yml scripts/*.sh 2>/dev/null \
    | sort -u | grep -v '^debian/circle$' || true)

# A pattern that matches nothing is a broken pattern, not a repository that
# names no paths — this one names ten. Without a floor, breaking the regex
# turns the check into a no-op that reports success, which is how coverage
# leaves quietly. `|| true` above is what makes this reachable: under
# `pipefail` a grep matching nothing kills the assignment, and the script
# exited with status 1 and no output at all.
[ -n "$named" ] || fail \
"the configuration sweep matched nothing, so its pattern is broken.
Packaging and CI name paths in this repository; a sweep finding none of them
is not a passing check, it is a check that has stopped looking." 
absent=""
for path in $named; do
    [ -e "$path" ] || absent="$absent $path"
done
echo "packaging: $(echo $named | wc -w) literal paths named by config"
[ -z "$absent" ] || fail \
"packaging or CI names paths this checkout does not contain:
$(for a in $absent; do echo "  $a"; done)
A path named by configuration and reached by no build is invisible until the
job that names it runs."


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
# built from a local path resolves perfectly on the machine that committed it
# and fails only where it matters.
#
# A lockfile entry with no `source` is built from a local path. The substrate
# resolves from crates.io now, so this checkout is the only crate that may
# appear without one — a second name means a path dependency crept back in,
# and a path dependency here would make a release build prefer a working tree
# next door over the version the manifest names.
#
# This is the file that gets staged without being read, because a lockfile
# diff always looks like noise, which is why it wants a check and not a habit.
local_pkgs=$(awk '
    /^\[\[package\]\]/ { name=""; src=0; next }
    /^name = /         { gsub(/"/,""); name=$3; next }
    /^source = /       { src=1; next }
    /^$/               { if (name != "" && src == 0) print name; name="" }
    END                { if (name != "" && src == 0) print name }
' Cargo.lock)

echo "lockfile: local crates $(echo $local_pkgs)"
stray=$(comm -23 <(echo "$local_pkgs" | sort -u) <(printf 'circle\n'))
[ -z "$stray" ] || fail \
"Cargo.lock names crates built from a local path:
$(echo "$stray" | sed 's/^/  /')
Only circle itself may resolve that way. Everything else comes from crates.io,
so these resolve here and nowhere else — a path dependency has come back."

# --- Files named only by packaging ------------------------------------------
#
# `just install` and the flatpak manifest name one icon per declared size.
# Nothing else reaches those paths: no build reads them, no test opens them,
# so a rename or a deletion is invisible until somebody packages a release.
# That is how this repository shipped a HEAD whose `include_bytes!` named a
# file nobody had committed — the same shape with a different extension.
#
# The set is read out of the justfile's own declarations rather than repeated
# here, so adding a size to `icon-sizes` extends this check by itself. In a
# clean clone and in a `git archive` extraction — the only two places this
# runs — existing and being committed are the same thing.
appid=$(sed -n "s/^appid := '\(.*\)'/\1/p" justfile)
# `|| true` for the same reason: without it a missing `icon-dir` line kills
# this assignment under `pipefail`, and the explicit guard below — written for
# exactly that case — never runs. An informative failure message placed after
# a pipeline that aborts the script is unreachable.
icon_dir=$(sed -n 's/^icon-dir := //p' justfile | grep -oE "'[^']*'" | tr -d "'" | paste -sd/ || true)
sizes=$(sed -n "s/^icon-sizes := '\(.*\)'/\1/p" justfile)
[ -n "$appid" ] && [ -n "$icon_dir" ] && [ -n "$sizes" ] \
    || fail "cannot read appid, icon-dir or icon-sizes out of the justfile"

missing=""
for size in $sizes; do
    [ -f "$icon_dir/$size/apps/$appid.png" ] || missing="$missing $icon_dir/$size/apps/$appid.png"
done
for svg in "$icon_dir/scalable/apps/$appid.svg" "$icon_dir/symbolic/apps/$appid-symbolic.svg"; do
    [ -f "$svg" ] || missing="$missing $svg"
done

echo "packaging: $(echo $sizes | wc -w) icon sizes plus scalable and symbolic"
[ -z "$missing" ] || fail \
"packaging names files this checkout does not contain:
$(for m in $missing; do echo "  $m"; done)
Nothing builds or tests these, so only packaging a release would notice.
Either commit them, or stop naming them in the justfile and the manifest."

echo "preflight: this checkout is what it claims to be"
