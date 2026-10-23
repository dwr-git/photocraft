#!/usr/bin/env bash
# Builds PhotoCraft Studio: the browser app plus the assistant panel and its PHP, as one folder
# ready for the server, zipped:  $DIST/photocraft-studio-<version>.zip  →  photocraft/
#
# Usage: studio/build/package.sh [--skip-build]
#   --skip-build  reuse the last trunk build (dist/web, dist/web-boot)
#
# Needs what packaging/web/package.sh needs (trunk, the wasm32 target) plus python3 and zip.
# CI runs it in .github/workflows/studio.yml; nobody needs Rust on the server.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
# shellcheck source=../../packaging/env.sh
. "$ROOT/packaging/env.sh"

"$ROOT/packaging/web/package.sh" "$@"

SITE="$CARGO_TARGET_DIR/web-package/photocraft-web-$VERSION"
[ -f "$SITE/index.html" ] || { echo "error: $SITE/index.html missing" >&2; exit 1; }

OUT="$CARGO_TARGET_DIR/studio-package"
rm -rf "$OUT"
mkdir -p "$OUT"
cp -R "$SITE" "$OUT/photocraft"
APP="$OUT/photocraft"

# The panel: studio/ next to index.html.
mkdir -p "$APP/studio"
cp "$ROOT/studio/web/studio.js" "$ROOT/studio/web/studio.css" "$ROOT/studio/web/config.js" "$APP/studio/"
cp "$ROOT/studio/web/.htaccess" "$APP/studio/"
python3 "$ROOT/studio/build/make_labels.py" "$APP/studio/ru-labels.json"

# The assistant's PHP: api/ next to index.html.
mkdir -p "$APP/api"
cp "$ROOT"/studio/api/*.php "$APP/api/"
cp "$ROOT/studio/api/.htaccess" "$APP/api/"

# Docs for whoever puts it on the server.
cp "$ROOT/studio/README.ru.md" "$APP/README.ru.md"
cp "$ROOT/studio/nginx.conf.example" "$APP/nginx.conf.example"

# The panel's files keep their names between releases: a version in the URL makes browsers
# fetch the new ones after an update (config.js is meant to be edited on the server, so it is
# served with no-cache instead, see studio/web/.htaccess and nginx.conf.example).
STAMP="${PHOTOCRAFT_BUILD_SHA:0:8}"
[ -n "$STAMP" ] || STAMP="$VERSION"
sed -i.bak \
  -e "s#href=\"studio/studio.css\"#href=\"studio/studio.css?v=$STAMP\"#" \
  -e "s#src=\"studio/studio.js\"#src=\"studio/studio.js?v=$STAMP\"#" \
  "$APP/index.html"
rm -f "$APP/index.html.bak"
grep -q "studio/studio.js?v=$STAMP" "$APP/index.html" || { echo "error: the page does not load the assistant panel" >&2; exit 1; }

ZIP="$DIST/photocraft-studio-$VERSION.zip"
rm -f "$ZIP"
(cd "$OUT" && zip -qr9 "$ZIP" photocraft)
echo "wrote $ZIP"
ls -lh "$ZIP"
