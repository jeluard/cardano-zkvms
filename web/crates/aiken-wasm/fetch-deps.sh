#!/usr/bin/env bash
# Downloads the Aiken stdlib and fuzz library sources needed for compilation.
# These .ak files are embedded into the WASM binary via include_str!().
set -euo pipefail

STDLIB_VERSION="v2.2.0"

cd "$(dirname "$0")"

echo "==> Fetching stdlib ${STDLIB_VERSION}…"
rm -rf stdlib && mkdir -p stdlib
curl -sL \
  -H "Accept: application/vnd.github+json" \
  -H "X-GitHub-Api-Version: 2022-11-28" \
  "https://api.github.com/repos/aiken-lang/stdlib/tarball/${STDLIB_VERSION}" \
  -o stdlib.tar
tar -xf stdlib.tar --strip-components 1 -C stdlib
rm stdlib.tar
echo "   stdlib extracted to ./stdlib/"

echo "==> Done. Ready to build."
