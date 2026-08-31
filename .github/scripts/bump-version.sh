#!/usr/bin/env bash
#
# Compute the next semver from the latest v*.*.* tag, write it into the
# workspace, and print it. Usage: bump-version.sh <patch|minor|major>
set -euo pipefail

bump="${1:?usage: bump-version.sh patch|minor|major}"
case "$bump" in
  patch | minor | major) ;;
  *)
    echo "unknown bump '$bump'" >&2
    exit 1
    ;;
esac

latest="$(git tag --list 'v*.*.*' --sort=-v:refname | head -n1 || true)"
if [ -z "$latest" ]; then
  latest="v0.0.0"
fi
ver="${latest#v}"
IFS=. read -r major minor patch <<<"$ver"
major="${major:-0}"
minor="${minor:-0}"
patch="${patch:-0}"

case "$bump" in
  major)
    major=$((major + 1))
    minor=0
    patch=0
    ;;
  minor)
    minor=$((minor + 1))
    patch=0
    ;;
  patch)
    patch=$((patch + 1))
    ;;
esac

version="$major.$minor.$patch"
tag="v$version"

if git rev-parse "$tag" >/dev/null 2>&1; then
  echo "tag $tag already exists" >&2
  exit 1
fi

python3 - "$version" <<'PY'
import json, pathlib, re, sys

version = sys.argv[1]
root = pathlib.Path(".")

cargo_path = root / "Cargo.toml"
cargo = cargo_path.read_text()

def sub_ws(match):
    body = match.group(1)
    new, n = re.subn(
        r'(?m)^(version\s*=\s*)"[^"]*"',
        rf'\1"{version}"',
        body,
        count=1,
    )
    if n != 1:
        raise SystemExit("could not find version in [workspace.package]")
    return "[workspace.package]" + new

cargo2, n = re.subn(
    r"\[workspace\.package\](.*?)(?=\n\[|\Z)",
    sub_ws,
    cargo,
    count=1,
    flags=re.S,
)
if n != 1:
    raise SystemExit("could not find [workspace.package]")
cargo_path.write_text(cargo2)

pkg_path = root / "app/projects/erno-angular/package.json"
pkg = json.loads(pkg_path.read_text())
pkg["version"] = version
pkg_path.write_text(json.dumps(pkg, indent=2) + "\n")
PY

if [ -n "${GITHUB_OUTPUT:-}" ]; then
  echo "version=$version" >> "$GITHUB_OUTPUT"
  echo "tag=$tag" >> "$GITHUB_OUTPUT"
fi
echo "$version"
