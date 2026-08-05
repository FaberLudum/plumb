#!/usr/bin/env bash
#
# Fails unless the compiler in front of cargo is the one rust-toolchain.toml
# names.
#
# Installing a toolchain is not the same as running it. An override, a stale
# rustup, or a renamed key in rust-toolchain.toml would each leave a job
# building on whatever the runner image happens to ship — green, and testing a
# different compiler from the one the crates claim to support.
#
# A file rather than an inline `run:` because two jobs need it, and the copy
# that is not being read is the one that goes stale.
#
# Invoked as `bash .github/scripts/assert-toolchain.sh` from the repository
# root, so no execute bit is needed and Windows does not have to carry one.

set -euo pipefail

toolchain_file="rust-toolchain.toml"

# The trailing `.*` matters on Windows: it absorbs the carriage return a CRLF
# checkout leaves on the line, which would otherwise end up inside `declared`
# and fail every comparison below for a reason that has nothing to do with the
# toolchain.
declared=$(sed -n 's/^[[:space:]]*channel[[:space:]]*=[[:space:]]*"\([^"]*\)".*/\1/p' "$toolchain_file")

if [ -z "$declared" ]; then
  echo "::error::no channel found in $toolchain_file; this check would pass without checking anything"
  exit 1
fi

active=$(rustc --version)

# `1.97` must match `rustc 1.97.1`, and `1.97.1` must match it too, but `1.9`
# must not: hence two patterns, one anchored on the space and one on the dot.
#
# A non-numeric channel — `stable`, `nightly` — fails here. That is deliberate
# and it fails loudly: this repository pins an exact version, and the day it
# stops doing so, this script is one of the things that has to be rewritten
# rather than something that should quietly start accepting anything.
case "$active" in
  "rustc $declared "* | "rustc $declared."*) ;;
  *)
    echo "::error::$toolchain_file asks for $declared, but $active is what will run"
    exit 1
    ;;
esac

echo "$active against $toolchain_file channel $declared"
