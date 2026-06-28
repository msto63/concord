#!/usr/bin/env bash
# Install Concord's local git hooks. The pre-push hook runs the version-discipline
# check before every push, so VERSION / CHANGELOG / `concord version` can never drift —
# entirely locally, with no CI service and no cost.
#   bash scripts/install-hooks.sh
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
