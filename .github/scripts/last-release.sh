#!/usr/bin/env bash
# The newest released version among the git tags given on stdin, one per
# line: only `v<x.y.z>` tags count, so a tag such as `vnext` or `v1` is never
# taken for a release. Prints nothing when there is none.
#
# Usage: git tag -l | last-release.sh
set -euo pipefail

grep -E '^v[0-9]+\.[0-9]+\.[0-9]+$' | sed 's/^v//' | sort -t. -k1,1n -k2,2n -k3,3n | tail -1 || true
