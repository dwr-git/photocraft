#!/usr/bin/env python3
"""Russian names for the assistant's command search.

Reads PhotoCraft's Russian UI catalog (crates/ui-egui/src/i18n/ru.tsv) and writes
studio/web/ru-labels.json:

    {"text": {"Curves…": "Кривые…", ...}, "id": {"file.saveAs": "Сохранить как…", ...}}

`text` translates menu labels and menu-path segments; `id` holds per-command overrides
(the catalog's `@id` rows: id in the source column). The assistant panel uses both to show and search commands in
Russian. Run by studio/build/package.sh; no third-party packages needed.
"""

import json
import sys
from pathlib import Path


def unescape(s: str) -> str:
    out, i = [], 0
    while i < len(s):
        c = s[i]
        if c == "\\" and i + 1 < len(s):
            nxt = s[i + 1]
            out.append({"n": "\n", "t": "\t", "\\": "\\"}.get(nxt, "\\" + nxt))
            i += 2
            continue
        out.append(c)
        i += 1
    return "".join(out)


def main() -> int:
    root = Path(__file__).resolve().parents[2]
    src = root / "crates/ui-egui/src/i18n/ru.tsv"
    dst = Path(sys.argv[1]) if len(sys.argv) > 1 else root / "studio/web/ru-labels.json"
    text: dict[str, str] = {}
    ids: dict[str, str] = {}
    for line in src.read_text(encoding="utf-8").splitlines():
        if not line.strip() or line.lstrip().startswith("#"):
            continue
        cols = line.split("\t")
        if len(cols) < 3:
            continue
        ctx, source, translation = cols[0], unescape(cols[1]), unescape(cols[2])
        if not translation:
            continue
        if ctx == "":
            text.setdefault(source, translation)
        elif ctx == "@id":
            # A command id whose menu item reads differently from its English twin.
            ids[source] = translation
    dst.parent.mkdir(parents=True, exist_ok=True)
    dst.write_text(json.dumps({"text": text, "id": ids}, ensure_ascii=False, separators=(",", ":")), encoding="utf-8")
    print(f"{dst}: {len(text)} labels, {len(ids)} command overrides")
    return 0


if __name__ == "__main__":
    sys.exit(main())
