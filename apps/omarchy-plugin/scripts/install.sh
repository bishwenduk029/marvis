#!/usr/bin/env bash
# Symlink this plugin into the Omarchy plugin directory and validate it.
set -euo pipefail

dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
target="${HOME}/.config/omarchy/plugins/dev.marvis.app"

mkdir -p "$(dirname "$target")"
if [[ -L "$target" || -e "$target" ]]; then
  if [[ -L "$target" ]]; then
    rm "$target"
  else
    echo "Refusing to replace non-symlink $target" >&2
    exit 1
  fi
fi
ln -s "$dir" "$target"
echo "Linked $target -> $dir"

if command -v omarchy >/dev/null 2>&1; then
  omarchy plugin validate dev.marvis.app || true
  echo "Restart the shell to load it: omarchy shell restart"
fi
