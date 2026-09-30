#!/usr/bin/env bash
# What CI would say about formatting and lints, on all three platforms,
# before anything is pushed.
#
# Nearly every release since v0.8.0 went red on its first CI run, and every
# time it was something this machine never looked at: a file rustfmt had not
# seen, or code that only compiles for Windows or macOS (v0.18.0: a field only
# the X11 and Win32 windows read was dead code on macOS). Clippy checks
# without linking, so the other two platforms can be linted from here.
#
# Two gaps, said rather than hidden:
# - macOS skips fontelle-app and fontelle-net. They reach `ring`, whose build
#   script compiles C against the macOS SDK, which is not on this machine.
# - Tests are not run here. CI runs them on all three; push `main` when a
#   chunk lands, not only at release, so a runner-only failure turns up days
#   before the release instead of on the release commit.
#
# Run by `.githooks/pre-push`; `FONTELLE_SKIP_PREFLIGHT=1 git push` skips it.
set -uo pipefail
cd "$(git rev-parse --show-toplevel)"

failed=()
step() {
    local name=$1
    shift
    echo "preflight: $name"
    if ! "$@"; then
        failed+=("$name")
    fi
}
installed() {
    rustup target list --installed 2>/dev/null | grep -qx "$1"
}

step "fmt" cargo fmt --all -- --check
step "clippy (this machine)" \
    cargo clippy -q --workspace --all-targets -- -D warnings

# The GNU toolchain rather than MSVC's: it builds `ring` with mingw, and what
# clippy checks is `cfg(windows)`, which both share.
if installed x86_64-pc-windows-gnu; then
    step "clippy (Windows)" \
        cargo clippy -q --workspace --all-targets --target x86_64-pc-windows-gnu \
        -- -D warnings
else
    echo "preflight: skipping Windows — rustup target add x86_64-pc-windows-gnu"
fi

if installed aarch64-apple-darwin; then
    step "clippy (macOS, without fontelle-app and fontelle-net)" \
        cargo clippy -q --workspace --exclude fontelle-app --exclude fontelle-net \
        --all-targets --target aarch64-apple-darwin -- -D warnings
else
    echo "preflight: skipping macOS — rustup target add aarch64-apple-darwin"
fi

if ((${#failed[@]})); then
    printf 'preflight: FAILED — %s\n' "${failed[@]}"
    exit 1
fi
echo "preflight: all clear"
