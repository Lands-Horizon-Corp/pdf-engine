#!/usr/bin/env bash
# Build and push the multi-arch image one platform at a time (parallel LTO links of this crate
# need more memory than a typical Docker VM has), then join them under every tag.
# Also leaves per-architecture tags (<version>-amd64, <version>-arm64).
#
#   .public/scripts/release-image.sh <image> <version> <platform,platform,...> <tag> [<tag>...]
set -euo pipefail

image=$1 version=$2 platforms=$3
shift 3
tags=("$@")

# Each platform is pushed as <version>-<arch> (works with Docker Desktop's default builder,
# which can't push by digest), then combined into the multi-arch tags.
arch_images=()
for platform in ${platforms//,/ }; do
    arch_tag="$image:$version-${platform##*/}"
    echo "==> building $platform as $arch_tag" >&2
    docker buildx build --platform "$platform" --build-arg VERSION="$version" -t "$arch_tag" --push .
    arch_images+=("$arch_tag")
done

echo "==> tagging ${tags[*]}" >&2
tag_args=()
for t in "${tags[@]}"; do tag_args+=(-t "$image:$t"); done
docker buildx imagetools create "${tag_args[@]}" "${arch_images[@]}"
