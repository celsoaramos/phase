#!/usr/bin/env bash
# Traz as tags/main do phase-rs e as branches card/* deste fork para o checkout em $1.
set -euo pipefail
cd "$1"
git remote get-url upstream >/dev/null 2>&1 || git remote add upstream "${UPSTREAM:-https://github.com/phase-rs/phase.git}"
git fetch -q --tags upstream main
git fetch -q origin '+refs/heads/card/*:refs/remotes/origin/card/*'
