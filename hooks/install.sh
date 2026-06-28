#!/usr/bin/env bash
# Concord automation install — wires the hooks + statusline into ~/.claude/settings.json.
# Backs up first, merges via python (preserves all existing keys), verifies. Idempotent.
set -euo pipefail
S="$HOME/.claude/settings.json"
H="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"   # this hooks/ directory, wherever it lives
[ -f "$S" ] || { echo "{}" > "$S"; }
cp "$S" "$S.concord-bak.$(date +%s)"
echo "Backup: $S.concord-bak.*"
python3 - "$S" "$H" <<'PY'
import sys, json
spath, H = sys.argv[1], sys.argv[2]
with open(spath) as f: cfg = json.load(f)
cfg["statusLine"] = {"type": "command", "command": f"{H}/statusline.sh"}
hooks = cfg.setdefault("hooks", {})
def ent(cmd, matcher=None):
    h = {"hooks": [{"type": "command", "command": cmd}]}
    if matcher is not None: h["matcher"] = matcher
    return [h]
hooks["SessionStart"]     = ent(f"{H}/session-start.sh")
hooks["UserPromptSubmit"] = ent(f"{H}/user-prompt.sh")
hooks["PostToolUse"]      = ent(f"{H}/post-tool.sh")
hooks["PreToolUse"]       = ent(f"{H}/pre-tool.sh", "Edit|Write|MultiEdit|NotebookEdit|Bash")
with open(spath, "w") as f: json.dump(cfg, f, indent=2, ensure_ascii=False)
print("merged statusLine + SessionStart/UserPromptSubmit/PostToolUse/PreToolUse hooks")
PY
python3 -c 'import json;d=json.load(open("'"$S"'"));assert d.get("statusLine");assert set(("SessionStart","UserPromptSubmit","PostToolUse","PreToolUse"))<=set(d.get("hooks",{}));print("verified: valid JSON, all hooks present, existing keys kept:", [k for k in d if k not in ("hooks","statusLine")])'
echo "DONE. New / restarted sessions pick up the hooks. To revert: bash $H/uninstall.sh"
