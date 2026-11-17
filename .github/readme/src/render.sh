#!/bin/sh
# Renders the README images from the HTML next to this script with headless Chrome (Mac).
# Run from anywhere: sh .github/readme/src/render.sh
set -e
cd "$(dirname "$0")"
CHROME="/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"

shot() { # name width height
  "$CHROME" --headless=new --disable-gpu --hide-scrollbars --force-device-scale-factor=2 \
    --default-background-color=00000000 --virtual-time-budget=8000 --allow-file-access-from-files \
    --window-size="$2,$3" --screenshot="$PWD/../$1.png" "file://$PWD/$1.html" 2>/dev/null
}

shot banner 1280 400
shot hero 1280 700

# Link previews: the website's og.png and GitHub's social preview (Settings → Social preview).
og() { # out width height
  "$CHROME" --headless=new --disable-gpu --hide-scrollbars --force-device-scale-factor=1 \
    --virtual-time-budget=8000 --window-size="$2,$3" --screenshot="$1" "file://$PWD/og.html" 2>/dev/null
}
og "$PWD/../../../apps/website/public/og.png" 1200 630
og "$PWD/../social.png" 1280 640
