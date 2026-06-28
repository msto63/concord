# Concord — Roadmap

Concord = dateibasierte Mehr-Session-KI-Koordination (siehe `README.md`). Dieses Repo ist die
**Heimat der Weiterentwicklung** — und Concord soll **sich selbst** damit koordinieren (Dogfooding).

## Stand
- ✅ **Hooks** (statusline · auto-register/heartbeat · status-injection · lease/merge-enforcement)
- ✅ **`concord`-CLI** (`start`/`stop`/`pause`/`resume`/`dash`) · Identität via `CONCORD_ID` ·
  **Worktree-Konvention `<repo>-<id>`** · **READY/GO-Dispatch-Handshake** · Start **im aktuellen Terminal**
- 🔄 **§8 Multi-Projekt** (begonnen): Skripte lesen jetzt `CONCORD_DIR`/`CONCORD_SYNC`/`CONCORD_PROJECT` aus
  der Env (sonst Ableitung `<repo>-coord` / `<repo>-SESSION-SYNC.md`); `concord` exportiert sie beim Start.
  **Offen:** `session-start.sh`-Meldungspfad ent-hartkodieren · `AIS_*`-Legacy-Fallbacks später entfernen ·
  ais auf dieses Repo umstellen (s. u.) · `concord init <ids…>`.

## Offen
- ⬜ **§6 Strukturiertes Board** (`board.jsonl` + `concord board`) — Arbeitspakete → Tasks mit
  Status × Priorität × Agent; K setzt Prio, Owner kippt Status.
- ⬜ **§7 Concord-MCP-Server** — register/claim/merge-lock/status/board als typisierte Tools.
- ⬜ **§9 Günstigere Inter-Agent-Kommunikation** (mike-Idee 2026-06-28). *Problem:* der Prosa-Kanal
  (`SESSION-SYNC.md`) wächst monoton (>17k Zeilen) und **jede** Session liest/injiziert daraus → teuer.
  *Bausteine:*
  - **Strukturierte Nachrichten** statt freiem Prosa-Block: feste Felder `{from,to,type,ref,body}` mit
    Typ-Enum (`READY|GO|ACK|DONE|BLOCKED|DESIGN|DECISION|PR|NUDGE`) + **kurzem** NL-Body. Parsebar, klein —
    *aber kein kryptischer Code* (LLM-Zuverlässigkeit braucht etwas natürliche Sprache).
  - **Per-Empfänger-Zustellung (Inbox):** je Session eine eigene Queue (`inbox/<id>.jsonl`); eine Session
    liest **nur ihre** Nachrichten, nicht den ganzen gemeinsamen Kanal. **Der größte Token-Spar-Hebel.**
  - **Delta-/nur-Neu-Injektion:** Hook injiziert nur ungesehene Nachrichten für die Session (Marker je id).
  - **Referenz statt Wiederholung:** auf IDs verweisen (PR#, Task-id, Lease) statt Kontext zu wiederholen.
  - **Menschen-Log behalten:** ein lesbarer Prosa-/Audit-Log für mike/K bleibt; die *Agenten* reden über
    die kompakte strukturierte Queue. (`coord.sh sync` ist der erste Schritt dahin.)

## Dogfooding (Concord-für-Concord)
Sobald §8 rund ist: `concord-coord/` + `concord-SESSION-SYNC.md` + Worktrees `concord-a … concord-k` —
Concords eigene Entwicklung wird mit Concord koordiniert.

## ais-Migration (deliberat, nicht den laufenden ais-Betrieb brechen)
1. Globale Hooks (`~/.claude/settings.json`) auf `~/Projects/concord/hooks/*` umzeigen.
2. ais-Sessions mit `CONCORD_DIR=…/ais-coord CONCORD_SYNC=…/ais-SESSION-SYNC.md` (oder via `concord` aus
   dem ais-Root abgeleitet) starten.
3. ais' `tools/coord.sh` → Symlink auf `~/Projects/concord/bin/coord.sh` (eine Quelle der Wahrheit).
