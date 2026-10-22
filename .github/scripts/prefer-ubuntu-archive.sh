#!/usr/bin/env bash
set -euo pipefail

# GitHub's Azure Ubuntu mirror can download Playwright's many WebKit packages
# too slowly for the bounded install step. Prefer Canonical's archive and keep
# the runner's Azure mirror as a fallback.
if [ -f /etc/apt/apt-mirrors.txt ]; then
  printf 'https://archive.ubuntu.com/ubuntu/\tpriority:1\nhttp://azure.archive.ubuntu.com/ubuntu/\tpriority:2\n' \
    | sudo tee /etc/apt/apt-mirrors.txt >/dev/null
elif [ -f /etc/apt/sources.list.d/ubuntu.sources ]; then
  sudo sed -i \
    's#http://azure.archive.ubuntu.com/ubuntu#https://archive.ubuntu.com/ubuntu#g' \
    /etc/apt/sources.list.d/ubuntu.sources
else
  echo '::error::Ubuntu apt sources were not found' >&2
  exit 1
fi
