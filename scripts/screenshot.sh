#!/usr/bin/env bash
# Take a *region* screenshot with the native tool and open it in screenshit.
# Bind this script to a hotkey (e.g. PrintScreen) in your desktop settings.
#
# Note: running `screenshit` with no arguments already captures the FULL
# screen by itself; this script is for when you want native region selection.
#
# Override the editor binary with:  SCREENSHIT_BIN=/path/to/screenshit
set -euo pipefail

APP="${SCREENSHIT_BIN:-screenshit}"
FILE="$(mktemp --suffix=.png)"
trap 'rm -f "$FILE"' EXIT

have() { command -v "$1" >/dev/null 2>&1; }

if [ -n "${WAYLAND_DISPLAY:-}" ]; then
    if have grim && have slurp; then
        grim -g "$(slurp)" "$FILE"
    elif have gnome-screenshot; then
        gnome-screenshot -a -f "$FILE"
    elif have spectacle; then
        spectacle -b -n -r -o "$FILE"
    else
        echo "No screenshot tool found. Install grim+slurp, gnome-screenshot, or spectacle." >&2
        exit 1
    fi
else
    if have maim; then
        maim -s "$FILE"
    elif have gnome-screenshot; then
        gnome-screenshot -a -f "$FILE"
    elif have spectacle; then
        spectacle -b -n -r -o "$FILE"
    elif have scrot; then
        scrot -s -o "$FILE"
    else
        echo "No screenshot tool found. Install maim, gnome-screenshot, spectacle, or scrot." >&2
        exit 1
    fi
fi

# Region select can be cancelled -> empty file; don't open the editor then.
[ -s "$FILE" ] || exit 0

trap - EXIT
"$APP" "$FILE"
rm -f "$FILE"
