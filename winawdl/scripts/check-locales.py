"""Check translation coverage and numbered placeholders; no network access."""
import json
import re
from pathlib import Path

root = Path(__file__).resolve().parents[2] / "locales"
codes = ["zh-CN", "zh-HK", "zh-TW", "ja", "fr", "ru", "es"]
source = json.loads((root / "en.json").read_text(encoding="utf-8"))
reference = json.loads((root / "zh-CN.json").read_text(encoding="utf-8"))
fields = lambda text: set(re.findall(r"\{(\d+)(?::[^}]*)?\}", text))
errors = []
for code in codes:
    catalog = json.loads((root / f"{code}.json").read_text(encoding="utf-8"))
    for key in set(reference) | set(source.values()):
        value = catalog.get(key)
        if not isinstance(value, str) or not value:
            errors.append(f"{code}: missing {key!r}")
        elif fields(key) != fields(value):
            errors.append(f"{code}: placeholder mismatch in {key!r}")
if errors:
    raise SystemExit("\n".join(errors))
print(f"{len(codes)} catalogs: {len(reference)} entries each; placeholders match")
