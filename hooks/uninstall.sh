#!/usr/bin/env bash
# Concord automation uninstall — removes the Concord hooks + statusline from
# ~/.claude/settings.json (keeps everything else). Restores the newest backup if
# present, else surgically deletes only the Concord keys.
set -euo pipefail
S="$HOME/.claude/settings.json"
bak=$(ls -t "$S".concord-bak.* 2>/dev/null | head -1 || true)
if [ -n "${bak:-}" ]; then
  cp "$bak" "$S"; echo "restored backup: $bak"
else
  python3 - "$S" <<'PY'
import sys, json
spath = sys.argv[1]
with open(spath) as f: cfg = json.load(f)
cfg.pop("statusLine", None)
for k in ("SessionStart","SessionEnd","UserPromptSubmit","PostToolUse","PreToolUse"):
    cfg.get("hooks", {}).pop(k, None)
if cfg.get("hooks") == {}: cfg.pop("hooks", None)
with open(spath, "w") as f: json.dump(cfg, f, indent=2, ensure_ascii=False)
print("removed Concord statusLine + hooks")
PY
fi
echo "DONE. Concord-Automatisierung entfernt."
