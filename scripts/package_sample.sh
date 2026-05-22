#!/usr/bin/env bash
set -euo pipefail

mkdir -p dist
for manifest in examples/manifests/*.json; do
  name="$(basename "${manifest}" .json)"
  mkdir -p "dist/${name}"
  cp "${manifest}" "dist/${name}/manifest.json"
  (cd dist && zip -r "${name}.zip" "${name}" >/dev/null)
  rm -rf "dist/${name}"
  echo "wrote dist/${name}.zip"
done