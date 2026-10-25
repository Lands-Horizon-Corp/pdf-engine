#!/usr/bin/env bash
# Ship everything in one go: bump the version, commit all changes, push, tag, and push the
# Docker image (via `make release`).
#
#   .public/scripts/push.sh --version=1.1.2 --message="fix: something"
#   make push VERSION=1.1.2 MESSAGE="fix: something"
#
# Bumps Cargo.toml and Cargo.lock, adds a CHANGELOG.md entry with the message, then
# `git add -A`, commits with the message, pushes the branch, and runs `make release`.
set -euo pipefail
cd "$(dirname "$0")/../.."

version="" message=""
for arg in "$@"; do
    case "$arg" in
        --version=*) version=${arg#--version=} ;;
        --message=*) message=${arg#--message=} ;;
        *) echo "unknown argument: $arg" >&2; exit 1 ;;
    esac
done

die() { echo "push: $*" >&2; exit 1; }
[ -n "$version" ] || die "--version=X.Y.Z is required"
[ -n "$message" ] || die "--message=\"...\" is required"
[[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || die "version must look like 1.2.3, got '$version'"

current=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
[ "$version" != "$current" ] || die "version is already $current; pick a new one"
git fetch -q --tags origin
! git rev-parse -q --verify "refs/tags/v$version" >/dev/null || die "tag v$version already exists"
docker info >/dev/null 2>&1 || die "Docker isn't running"

echo "==> $current -> $version" >&2
sed -i.bak "s/^version = \"$current\"/version = \"$version\"/" Cargo.toml && rm Cargo.toml.bak
awk -v v="$version" '
    prev == "name = \"pdf-engine\"" && /^version = / { $0 = "version = \"" v "\"" }
    { print; prev = $0 }
' Cargo.lock > Cargo.lock.tmp && mv Cargo.lock.tmp Cargo.lock

entry="## [$version] - $(date +%F)\n\n- $message\n"
awk -v e="$entry" '!done && /^## \[/ { printf "%s\n", e; done = 1 } { print }' CHANGELOG.md \
    > CHANGELOG.md.tmp && mv CHANGELOG.md.tmp CHANGELOG.md

git add -A
git commit -m "$message"
git push
make release VERSION="$version"
