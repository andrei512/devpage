#!/bin/sh
# devpage's installer for macOS and Linux, served by devpage.ai as
# https://devpage.ai/install.sh: it runs the install script the latest release
# carries, which downloads the binary for this system (docs/installing.md).
set -eu
url=https://github.com/andrei512/devpage/releases/latest/download/devpage-installer.sh
if command -v curl >/dev/null 2>&1; then
  script=$(curl --proto '=https' --tlsv1.2 -LsSf "$url")
else
  script=$(wget --https-only -qO- "$url")
fi
printf '%s\n' "$script" | sh -s -- "$@"
# The live session's program and the bun that runs it
# (docs/boxes/viewer/live-session/README.md): devpage is installed whether or
# not this succeeds.
devpage=$HOME/.local/bin/devpage
[ -x "$devpage" ] || devpage=$(command -v devpage || true)
if [ -z "$devpage" ] || ! "$devpage" setup; then
  echo >&2
  echo "devpage is installed and works." >&2
  echo "Setting up the live session (talking to your pages) failed, see the error above." >&2
  echo "To try again, run: devpage setup" >&2
fi
