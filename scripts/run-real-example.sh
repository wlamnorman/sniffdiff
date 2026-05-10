#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 4 ]]; then
  echo "usage: $0 <name> <repo-url> <commit> <limit>" >&2
  exit 2
fi

name="$1"
repo_url="$2"
commit="$3"
limit="$4"
target_dir="target/real-world-examples/$name"

mkdir -p "$(dirname "$target_dir")"

if [[ ! -d "$target_dir/.git" ]]; then
  rm -rf "$target_dir"
  git init -q "$target_dir"
  git -C "$target_dir" remote add origin "$repo_url"
else
  git -C "$target_dir" remote set-url origin "$repo_url"
fi

git -C "$target_dir" fetch --quiet --no-tags --depth=2 origin "$commit"
git -C "$target_dir" checkout --quiet --detach "$commit"

base="$(git -C "$target_dir" rev-parse "$commit^")"
head="$(git -C "$target_dir" rev-parse "$commit")"

echo "Example: $name"
echo "Repo: $repo_url"
echo "Base: $base"
echo "Head: $head"
echo ""

cargo run -- --repo "$target_dir" "$base..$head" --limit "$limit" --caller-preview-limit 4
