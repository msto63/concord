#!/usr/bin/env bash
# Install Concord's local git hooks:
#   - pre-push : version-discipline check (VERSION / CHANGELOG / `concord version`).
#   - pre-commit : F5 signature-contract gate — block a commit that changes an agreed
#                  `<file>:<symbol>` contract without renegotiation. The merge-lock has the
#                  same precondition (the coordinator-level gate); this catches it earlier.
# Entirely local, no CI service, no cost.   bash scripts/install-hooks.sh
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

hook="$ROOT/.git/hooks/pre-push"
cat > "$hook" <<'EOF'
#!/usr/bin/env bash
# Concord pre-push: enforce version discipline locally (installed by scripts/install-hooks.sh).
exec bash "$(git rev-parse --show-toplevel)/scripts/check-version.sh"
EOF
chmod +x "$hook"
echo "✓ installed pre-push hook → runs scripts/check-version.sh before every push"

pc="$ROOT/.git/hooks/pre-commit"
cat > "$pc" <<'EOF'
#!/usr/bin/env bash
# Concord pre-commit: F5 signature-contract gate. Fail-open — if the concord binary is not
# found, the commit proceeds (the merge-lock precondition is the airtight gate).
bin="$(command -v concord || true)"
[ -z "$bin" ] && for c in target/release/concord target/debug/concord; do [ -x "$c" ] && bin="$c" && break; done
[ -z "$bin" ] && exit 0
exec "$bin" contract-check
EOF
chmod +x "$pc"
echo "✓ installed pre-commit hook → runs \`concord contract-check\` (F5) before every commit"
