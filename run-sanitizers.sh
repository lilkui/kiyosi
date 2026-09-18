#!/usr/bin/env bash
set -euo pipefail

cd -- "$(dirname -- "${BASH_SOURCE[0]}")"

cmake --preset linux-sanitizers
cmake --build --preset linux-sanitizers
ctest --preset linux-sanitizers
