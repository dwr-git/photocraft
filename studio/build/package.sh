#!/usr/bin/env bash
# Builds AI-Photoshop: the browser app plus the assistant panel and its PHP, as one folder
# ready for the server, zipped:  $DIST/ai-photoshop-<version>.zip  →  ai-photoshop/
# The folder works under any name: every path in it is relative.
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
cp -R "$SITE" "$OUT/ai-photoshop"
APP="$OUT/ai-photoshop"
# Upstream hosting notes (they describe the original project); this folder has its own README.
rm -f "$APP/HOSTING.md"

# The panel: studio/ next to index.html.
mkdir -p "$APP/studio"
cp "$ROOT/studio/web/studio.js" "$ROOT/studio/web/studio.css" "$ROOT/studio/web/config.js" "$ROOT/studio/web/loader.js" "$APP/studio/"
cp "$ROOT/studio/web/.htaccess" "$APP/studio/"
cp "$ROOT/studio/web/favicon.png" "$APP/favicon.png"
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
  -e "s#src=\"studio/loader.js\"#src=\"studio/loader.js?v=$STAMP\"#" \
  "$APP/index.html"
rm -f "$APP/index.html.bak"
grep -q "studio/studio.js?v=$STAMP" "$APP/index.html" || { echo "error: the page does not load the assistant panel" >&2; exit 1; }
grep -q "studio/loader.js?v=$STAMP" "$APP/index.html" || { echo "error: the page does not load the start-up check" >&2; exit 1; }
# Nothing in the folder may load from another site: the app must work with no internet.
if grep -rIlE "(src|href)=\"(https?:)?//" "$APP" --include=*.html >/dev/null; then echo "error: a page loads something from another site" >&2; exit 1; fi
grep -q 'data-app-bytes="[1-9]' "$APP/index.html" || { echo "error: the page does not know the download size" >&2; exit 1; }

# Номер сборки (GitHub Actions передаёт AI_PHOTOSHOP_BUILD) и дата — в имени архива и внутри папки,
# чтобы было видно, какая версия скачана и какая стоит на сервере (панель помощника пишет её в шапке).
BUILD="${AI_PHOTOSHOP_BUILD:-local}"
BUILD_DATE="$(TZ=Europe/Minsk date +%Y-%m-%d)"
printf '{"build":"%s","date":"%s","commit":"%s"}\n' "$BUILD" "$BUILD_DATE" "$STAMP" >"$APP/studio/version.json"
ZIP="$DIST/ai-photoshop-build-$BUILD-$BUILD_DATE.zip"
rm -f "$ZIP"
(cd "$OUT" && zip -qr9 "$ZIP" ai-photoshop)
echo "wrote $ZIP"
ls -lh "$ZIP"
