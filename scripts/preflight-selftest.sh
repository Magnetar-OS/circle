#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-only
#
# Proves each preflight check fails on the input it exists for.
#
# Without this, the evidence that those checks work is a set of one-off
# experiments somebody ran once — which is a practice, and a practice is
# deletable exactly when nothing fails without it. Every check in preflight
# was written because reading was not enough; the same applies to the
# checks themselves.
#
# Row zero is a positive control and everything depends on it. A matrix of
# negative results is a matrix of unknowns until one row is known to pass:
# an earlier version of this reported all cases caught, which meant only that
# the harness failed identically in every one of them.
#
# Note on writing this file: preflight scans `scripts/*.sh`, so a literal
# path in a comment here is a path preflight will demand exists. The absent
# paths below are therefore assembled at runtime rather than written down.
set -uo pipefail

cd "$(dirname "$0")/.."
root=$(pwd)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

tree="$work/circle"
mkdir -p "$tree"
git archive --format=tar HEAD | tar -x -C "$tree"
cp "$root"/scripts/*.sh "$tree/scripts/"
for sibling in ../*/; do
    name=$(basename "$sibling")
    [ "$name" = "$(basename "$root")" ] && continue
    ln -sfn "$(cd "$sibling" && pwd)" "$work/$name"
done

# The tree comes from HEAD, but the scripts under test come from the working
# copy, so an edit to preflight is exercised before it is committed. Extracting
# both from HEAD made this test silently examine the *committed* preflight: a
# deliberately neutered check in the working tree was reported "ok" by every
# row. The confound was invisible until a check was broken on purpose to see
# whether the matrix would notice.
scripts_in() { cp "$root"/scripts/*.sh "$1/scripts/"; }
fresh() {
    rm -rf "$tree"; mkdir -p "$tree"
    git archive --format=tar HEAD | tar -x -C "$tree"
    scripts_in "$tree"
}
run()   { (cd "$tree" && ./scripts/preflight.sh 2>&1); }

failures=0

# --- Row zero: the positive control -----------------------------------------
if ! run >/dev/null 2>&1; then
    echo "BASELINE FAILED — a clean extraction of HEAD does not pass preflight."
    echo "Nothing below this line would mean anything, so the matrix is not run."
    run | tail -5
    exit 1
fi
echo "ok    baseline: a clean extraction of HEAD passes"

# --- Provenance: is the script under test the one being edited? --------------
#
# The negative control below proves a broken check is noticed, but it cannot
# prove *which copy* it broke: neutering happens after the scripts are placed,
# so a harness that placed the committed copy would disable that one and stay
# green. This asserts by content instead of by inference.
#
# Compared against the literal path rather than through the variable the
# harness uses, because a rewiring is exactly what changes the variable, and a
# control that follows the rewiring cannot see it.
if ! cmp -s scripts/preflight.sh "$tree/scripts/preflight.sh"; then
    echo "PROVENANCE FAILED — the extracted tree is not running the working copy"
    echo "of scripts/preflight.sh, so every row below tests some other version."
    exit 1
fi
echo "ok    provenance: the working copy of preflight is what is under test"

# --- Each check, against the input it exists for -----------------------------
# $1 label, $2 expected substring in the failure, $3 shell that breaks the tree
expect_failure() {
    local label="$1" expected="$2" break_it="$3" out
    fresh
    ( cd "$tree" && eval "$break_it" )
    out=$(run)
    if [ -z "$out" ]; then
        echo "FAIL  $label: produced no output at all"; failures=$((failures + 1)); return
    fi
    if run >/dev/null 2>&1; then
        echo "FAIL  $label: preflight passed when it should not have"; failures=$((failures + 1)); return
    fi
    case "$out" in
        *"$expected"*) echo "ok    $label" ;;
        *) echo "FAIL  $label: failed, but never named '$expected'"
           echo "$out" | sed 's/^/        /' | tail -4
           failures=$((failures + 1)) ;;
    esac
}

expect_failure "toolchain: manifest below the pinned channel" \
    "Raise both together" \
    "sed -i 's/^rust-version = \"1.98.1\"/rust-version = \"1.98.1\"/' Cargo.toml"

expect_failure "lockfile: names a crate CI cannot fetch" \
    "cosmic-ext-nib-text" \
    "printf '\n[[package]]\nname = \"cosmic-ext-nib-text\"\nversion = \"0.1.0\"\n' >> Cargo.lock"

expect_failure "packaging: a declared icon size has no file" \
    "does not contain" \
    "rm -f resources/icons/hicolor/256x256/apps/com.magnetaros.Circle.png"

expect_failure "packaging: a size is declared but was never drawn" \
    "does not contain" \
    "sed -i \"s/^icon-sizes := '/icon-sizes := '\$(printf '8x%s' 8) /\" justfile"

expect_failure "config: a file named by config and read by other checks is gone" \
    "does not contain" \
    "rm -f rust-toolchain.toml"

# The two below are not bad *inputs* — they are preflight breaking, and the
# question is whether it says so or dies quietly. Both used to die quietly:
# under `pipefail` a grep matching nothing killed the assignment, so the script
# exited 1 with no output, and the explicit guard written for the second case
# sat after the pipeline that aborted before reaching it.
expect_failure "self: a broken sweep pattern is not a passing check" \
    "stopped looking" \
    "sed -i \"s|-ohE '((res|-ohE 'ZZZ((res|\" scripts/preflight.sh"

expect_failure "self: the justfile no longer declares what packaging installs" \
    "cannot read appid" \
    "sed -i '/^icon-dir := /d' justfile"

echo
if [ "$failures" -eq 0 ]; then
    echo "preflight self-test: every check fails on the input it exists for"
else
    echo "preflight self-test: $failures check(s) did not fail as they should"
    exit 1
fi
