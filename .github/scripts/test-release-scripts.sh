#!/usr/bin/env bash
# Tests for the release scripts, run by CI on every pull request, so what
# decides a release is proven before a release depends on it.
set -uo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
failed=0

expect() {
    local what="$1" want="$2" got="$3"
    if [ "$want" = "$got" ]; then
        echo "ok    $what"
    else
        echo "FAIL  $what"
        echo "      want: $(printf '%q' "$want")"
        echo "      got:  $(printf '%q' "$got")"
        failed=1
    fi
}

version() {
    out="$(bash "$here/release-version.sh" "$@" 2>/dev/null)"
    echo "$out:$?"
}

expect "the first release is released" "release:0" "$(version 0.1.0 '')"
expect "0.0.0 is never released" "skip:0" "$(version 0.0.0 '')"
expect "an unchanged version is skipped" "skip:0" "$(version 0.1.0 0.1.0)"
expect "a higher version is released" "release:0" "$(version 0.2.0 0.1.0)"
expect "versions compare as numbers" "release:0" "$(version 0.10.0 0.9.0)"
expect "a higher major beats a higher minor" "release:0" "$(version 1.0.0 0.99.0)"
expect "a lower version fails" ":1" "$(version 0.1.0 0.2.0)"
expect "a lower patch fails" ":1" "$(version 0.2.1 0.2.10)"
expect "a pre-release is not a release version" ":1" "$(version 0.2.0-rc.1 0.1.0)"
expect "build metadata is not a release version" ":1" "$(version 0.2.0+b1 0.1.0)"
expect "main between releases is never released" "skip:0" "$(version 0.2.0-dev 0.1.0)"
expect "only -dev is skipped among pre-releases" ":1" "$(version 0.2.0-dev.1 0.1.0)"

dev() {
    bash "$here/dev-version.sh" "$@" 2>/dev/null
    echo "$?"
}

expect "a -dev above the last release passes" "0" "$(dev 0.2.0-dev 0.1.0)"
expect "a release version above the last passes" "0" "$(dev 0.2.0 0.1.0)"
expect "with no release yet anything x.y.z passes" "0" "$(dev 0.1.0 '')"
expect "main still on the released version fails" "1" "$(dev 0.1.0 0.1.0)"
expect "a -dev of the released version fails" "1" "$(dev 0.1.0-dev 0.1.0)"
expect "a -dev below the last release fails" "1" "$(dev 0.1.5-dev 0.2.0)"
expect "versions compare as numbers" "0" "$(dev 0.10.0-dev 0.9.0)"
expect "another pre-release is not main's form" "1" "$(dev 0.2.0-rc.1 0.1.0)"

last() {
    printf '%s\n' "$@" | bash "$here/last-release.sh"
}

expect "the newest x.y.z tag is the last release" "0.10.0" "$(last v0.1.0 v0.9.0 v0.10.0 v0.2.0)"
expect "tags that are not x.y.z are ignored" "0.1.0" "$(last vnext v1 v0.1.0 v0.2.0-rc.1 v0.3.0+b)"
expect "no release tag gives nothing" "" "$(last vnext other)"

changelog="$(mktemp)"
trap 'rm -f "$changelog"' EXIT
cat > "$changelog" <<'EOF'
# Changelog

## [Unreleased]

## [0.2.0] - 2026-12-01

### Added

- Two.

## [0.1.0] - 2026-11-30

- One.

## [0.0.9] - 2026-11-01

EOF

section() {
    out="$(bash "$here/changelog-section.sh" "$1" "$changelog" 2>/dev/null)"
    echo "$out:$?"
}

expect "a section is printed whole and alone" "$(printf '### Added\n\n- Two.'):0" "$(section 0.2.0)"
expect "the last section stops at the next heading" "- One.:0" "$(section 0.1.0)"
expect "a missing section fails" ":1" "$(section 0.3.0)"
expect "an empty section fails" ":1" "$(section 0.0.9)"
expect "a version is not matched by its prefix" ":1" "$(section 0.1)"

exit "$failed"
