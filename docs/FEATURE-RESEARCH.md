# Concord — Feature-Mining Research

> Stand: 2026-06-29. Recherche via GitHub (`gh search`/`gh repo view`/`gh api`) + Web
> (WebSearch/WebFetch), drei parallele Recherche-Achsen + Primärquellen-Verifikation.
> **Frage:** Welche *Features* anderer agentischer Coding-Tools könnten Concord verbessern?
> (Abgrenzung: [`COMPETITIVE-LANDSCAPE.md`](COMPETITIVE-LANDSCAPE.md) beantwortet „gibt es ein
> Äquivalent?" — dieses Dokument beantwortet „was davon übernehmen wir, und lohnt es sich?".)

## 0. Vergleichsraster — was Concord SCHON KANN (nicht erneut vorschlagen)

Concord (Rust, v0.5.0, self-hosting) hat bereits: Multi-Session-**Registry** + Heartbeat/TTL-Stale-Reclaim;
**enforced Area-Leases** auf **Pfad- UND Symbol-/AST-Ebene** (tree-sitter); **Singleton Merge-Lock**;
**Fencing-Tokens** (Floor = FS-Self-Check + Strong = daemon-mediated); **Push-Daemon** (notify,
per-Session-Inbox-Demux statt Polling); **MCP-Server** (typisierte Tools); **Launcher**
(start/dash/pause/resume/stop); **Multi-Projekt**; **Cross-Platform-Distribution** (cargo-dist/`curl|sh`);
**Versions-Disziplin**; **advisory Call-Graph-Konflikt-Warnung**; und einen **autonomen
Koordinator-Agenten „hub"** (kein menschlicher Overseer nötig).

**Bewertungs-Leitplanke (aus CLAUDE.md / VISION):** Concords Wert ist die **durchgesetzte Vertikale**
(Capability, Klassifikation, Provenienz, Rechenschaft) + ein **intelligenter autonomer Koordinator**.
Features werden danach sortiert, ob sie *diese* stärken (hoher, vision-treuer Wert) oder bloß *commodity/
bequem* sind. Maxime: **durchgesetzte Koordination > Bequemlichkeit**, **kein Reinventing** wo reife
Prior-Art taugt.

---

## 1. Das organisierende Leitmotiv der Recherche

Drei Befunde bestimmen die Priorisierung:

1. **Concords Alleinstellung ist *Enforcement*, nicht Mechanik.** Fast jeder Peer (Agent Mail, swarm-protocol,
   gnap, claude-presence) implementiert Leases als **advisory**. Concord ist das einzige mit Fencing + AST-Leases.
   → Die wertvollsten Features sind die, die Concords *Durchsetzung* **noch härter** machen (Harness-Boundary,
   Contracts, out-of-scope-Detection) — nicht weitere bequeme Kanban-Bretter.
2. **Der Harness selbst (Claude Code Hooks) bietet inzwischen echte Enforcement-Primitive**, die Concord
   noch nicht voll nutzt — v.a. `PreToolUse`+`mcp_tool`-Deny (Lease-Block am Keystroke), `Stop`/`PreCompact`
   (gegen „dunkle" Sessions), `SessionEnd`/`WorktreeRemove` (sauberes Release). Das ist Concords **billigster,
   vision-treuester Hebel** — eigene Infrastruktur existiert schon, es fehlt die Harness-Verdrahtung.
3. **Anthropic baut mit „Agent Teams" in Concords Raum** — aber **ohne enforced Leases, ohne Merge-Lock,
   Lead fix, one-team-per-session**. Bestätigt Concords Burggraben (durchgesetzte Vertikale + autonomer
   cross-worktree-hub) und liefert zugleich Muster zum Spiegeln (file-locked Task-Claim, Mailbox,
   `TeammateIdle`/`TaskCompleted`-Enforcement-Hooks).

Quellen: Claude Code Hooks-Referenz <https://code.claude.com/docs/en/hooks>; Agent Teams
<https://code.claude.com/docs/en/agent-teams>; Plan-Mode-„nicht-enforced"-Analyse
<https://blog.sondera.ai/p/claude-codes-plan-mode-isnt-read>.

---

## 2. Feature-Katalog (gruppiert nach Vision-Beitrag)

Legende **Wert**: 🟢 vision-stärkend (durchgesetzte Koordination / Koordinator-Intelligenz / Provenienz-Audit)
· 🟡 nützlich, teils commodity · ⚪ commodity/orthogonal. **Aufwand**: S/M/L.

### A. Harness-native Enforcement — Leases von *advisory* zu *hart* (🟢, höchster Hebel)

| # | Feature | Quelle | Wert | Fit | Aufwand |
|---|---|---|---|---|---|
| A1 | **`PreToolUse` + `mcp_tool`-Deny** — Hook auf `Edit\|Write\|MultiEdit` ruft Concords MCP-Tool, `permissionDecision:"deny"` blockt jeden Edit auf eine *nicht geleaste* Datei/Symbol **am Keystroke**, bevor das Tool läuft. | Claude Code Hooks (offiziell) | 🟢 | exzellent (nutzt vorhandenen MCP-Server + Lease-Store) | **M** |
| A2 | **`SessionEnd`-Hook → Auto-Release** Leases/Merge-Lock + Deregister bei sauberem Exit (Doku nennt „release a lock" explizit). Schrumpft das Fenster, in dem eine fertige Session noch Leases hält. | Claude Code Hooks | 🟢 | exzellent (`coord.sh release`); idempotent halten, ergänzt TTL-Reclaim | **S** |
| A3 | **`Stop`-Hook (block-to-continue)** — bei Turn-Ende prüfen „hältst du ein Lease mit offener Arbeit / eine un-ge-ACK-te hub-Direktive?", ggf. `additionalContext` injizieren + Stop verweigern. **Harness-nativer Kurz gegen „going dark".** | Claude Code Hooks | 🟢 | stark, aber sauberes Abbruch-Prädikat nötig (sonst Endlos-Turn) | **M** |
| A4 | **`PreCompact` + `SessionStart(source=compact)`** — vor Kompaktierung Lease/Merge-Lock/Direktiven-Stand dumpen, nach Reset als `additionalContext` re-injizieren. Schützt Protokoll-Gedächtnis über Compaction. | Claude Code Hooks | 🟢 | exzellent (vorhandener Coord-State + Injection-Pfad) | **S–M** |
| A5 | **`FileChanged` + `watchPaths`** — ersetzt den brüchigen handgerollten `stat -f %m … sleep 30`-Monitor-Loop (in CLAUDE.md als #1-Ursache dunkler Sessions benannt) durch einen harness-nativen Wake auf `SESSION-SYNC`/Registry. | Claude Code Hooks | 🟢 | exzellent (direkter Ersatz eines fragilen Mechanismus) | **S** |
| A6 | **`PostToolUse`(Edit\|Write) → Out-of-scope-Write-Detection** — post-hoc erkennen, dass eine Session *außerhalb* ihrer Leases geschrieben hat → als Policy-Verletzung loggen/rückrollen. (Power Loom macht genau das als „out-of-scope write detection" mit reverse-cherry-pick-Journaling.) | Claude Code Hooks; Power Loom (`shashankcm95/claude-power-loom`) | 🟢 | exzellent — gibt Leases *Zähne* + Audit, auch wenn A1 umgangen wird | **M** |
| A7 | **`WorktreeCreate`/`WorktreeRemove`-Hooks** — bei `isolation:"worktree"` Session auto-registrieren/Coord-Dir seeden bzw. alle Leases freigeben. | Claude Code Hooks | 🟡 | gut für SDK/worktree-Pfad; manuelle Worktrees via A2/SessionStart abgedeckt | **S–M** |
| A8 | **`SubagentStart`/`SubagentStop`** — Parent-Lease-Scope an Subagenten vererben; bei Stop verifizieren, dass kein out-of-lease-Write zurückfließt. | Claude Code Hooks | 🟡 | gut (komponiert mit per-Session-Lease-Kontext) | **M** |

> **Hinweis (verifiziert):** `SessionStart`/`Setup` feuern **bevor** MCP-Server verbunden sind — Enforcement-Hooks
> (A1) müssen daher auf `PreToolUse` liegen, nicht auf `SessionStart`. Plan-Mode ist **prompt-only, nicht enforced**
> (Router dispatcht Edit/Write/Bash identisch) — das *validiert* Concords `PreToolUse`-Deny+Fencing als die einzige
> echte Garantie. Quellen: Hooks-Doku; <https://blog.sondera.ai/p/claude-codes-plan-mode-isnt-read>.

### B. Koordinator-Intelligenz / Observability — hub von *Prosa-Leser* zu *Telemetrie-getrieben* (🟢/🟡)

| # | Feature | Quelle | Wert | Fit | Aufwand |
|---|---|---|---|---|---|
| B1 | **hub-Telemetrie-Layer auf Claude Codes nativem OpenTelemetry** (`CLAUDE_CODE_ENABLE_TELEMETRY=1`): Token-Burn (input/output/cacheRead/cacheCreation), Kosten, Tool-Spans, Permission-Reject-Events, Subagent-Spans nesten unter Parent, `session.id` an jedem Span. hub berechnet daraus pro Session: Burn-Rate, **Idle** (keine Spans für N min), **Looping** (repetitive Spans / kein Commit-Fortschritt), Reject-Stürme. | offiziell <https://code.claude.com/docs/en/agent-sdk/observability> · SigNoz/GeneralAnalysis-Guides | 🟢 | gut — `session.id`→Concord-id beim Launch mappen; hub konsumiert via MCP-Tool; macht „kein stilles Idlen" *messbar* statt selbstberichtet | **M** |
| B2 | **ccusage** — lokales CLI parst Claude-Code-JSONL zu Token/Kosten pro Session/Tag/5h-Block (kein Upload). 16.7k★, 2026-06-29 sehr aktiv. hub kann `--json` pro Worktree für „wer ist teuer". | `ryoppippi/ccusage`, <https://ccusage.com/> | 🟡 | sehr gut (reiner lokaler Read, kein Daemon); ergänzt B1 (Kosten vs. Live-Loop) | **S** |
| B3 | **Dark/stuck-session-Watchdog mit aktivem Alerting** — mehrstufiges Health-Monitoring, das stalled/zombie-Agenten erkennt **und an hub meldet** (nicht nur passiv Leases reclaimt). Gastown: Witness→Deacon→Dogs + `problems view`; agent-kanban markiert nach 2h offline. | Gastown (`gastownhall/gastown`, **16.1k★**) · agent-kanban (376★) | 🟢 | komponiert (Heartbeat-Daten existieren; Watcher emittiert Eskalation B-Eskalation/#E2 bei Miss/Lease-aber-still) | **M** |
| B4 | **Live-Fleet-Dashboard** — read-only Sicht (wer aktiv, was geleast, offene Eskalationen) für Operator/hub statt `coord.sh status`-Polling. Blueprint: Hook→HTTP→SQLite→WS→Vue. | `disler/claude-code-hooks-multi-agent-observability` (~1.5k★) · Gastown `gt feed` · `chaspy/agent-exporter` (Prometheus+MCP) | 🟡 | komponiert (Push-Daemon hat Event-Plumbing); Add-on, keine Invarianten-Änderung | **M–L** |
| B5 | **Burn-Rate-Prognose** — vorhersagen, welche Session ihr Quota mitten in einer Aufgabe erschöpft, Merges/Arbeit entsprechend re-sequenzieren. | Claude-Code-Usage-Monitor (8.3k★) | 🟡 | Muster für hub-Heuristik auf B1/B2-Daten (keine Dependency) | **S** |
| B6 | **Session-Replay / Full-Trace-Capture** — jede Request/Response (System-Prompts, Tool-Defs, Token) als JSONL + self-contained HTML-Replay; für Post-hoc-Audit „warum lief Session X aus dem Ruder". | claude-trace (`@mariozechner/claude-trace`, in `badlogic/lemmy`) | 🟡 | orthogonal zu Enforcement; nützlich als Audit-Artefakt am Session-Record (Provenienz) | **S** (wrappen) |

> **Insight (mehrfach belegt):** „Ein Agent der loopt, falsches Tool ruft oder abdriftet liefert **trotzdem HTTP 200**
> mit normaler Latenz/Tokens." → Health muss auf **Session-/Trace-Ebene** modelliert werden, nicht pro Request.
> Concord sollte die *Heuristik* nativ auf dem OTel-Strom bauen, **keine** SaaS-Plattform (Langfuse/AgentOps) als
> Dependency ziehen (off-vision, infra-schwer); Langfuse ist OTLP-nativer Fallback-Store, falls je nötig.
> Quellen: <https://latitude.so/blog/best-ai-agent-observability-tools-2026-comparison>, <https://www.morphllm.com/agent-observability>.

### C. Provenienz & Rechenschaft — Concords Vertikale auf die Koordinations-Schicht angewandt (🟢)

| # | Feature | Quelle | Wert | Fit | Aufwand |
|---|---|---|---|---|---|
| C1 | **Kryptografische Agenten-Identität + signierte Commits/PRs** — Ed25519-Keypair → Fingerprint + JWT; Identität persistiert über Task-Claims, Git-Commits, PR-Signaturen. Macht „wer hat gemergt/editiert" **non-repudiable**; Fencing-Token kann *signiert* statt nur monoton sein. | agent-kanban (`saltbo/agent-kanban`, 376★, *aus Doku abgeleitet*) | 🟢 stärkste Provenienz/Rechenschafts-Play | gut (Keypair unter vorhandener id; signiert SESSION-SYNC/coord-log/Merge-Lock; optional enforced signed-commits im Merge-Gate) | **M–L** |
| C2 | **Governed Shared Memory** — Memory-Fragmente tragen **immutable Provenienz** (beitragende Agenten, Zeitstempel, genutzte Ressourcen) + vier Governance-Dimensionen (Scope/Time/Provenance/Propagation) + **retrospektive Permission-Checks**. = Concords eigene Vertikale (Cap+Klassifikation+Provenienz+Audit) auf *Agenten-Memory* angewandt; die enforced Version von SESSION-SYNC. | arXiv 2606.24535 / 2505.18279 (konzeptionell, *nicht* drop-in) | 🟢 | exzellent — `coord.sh log` zu fencing-versioniertem, provenienz-gestempeltem Decision-Store ausbauen (Version=current via Fencing, Scope via Lease) | **M–L** |
| C3 | **Transaktionaler Audit-Envelope pro Agent-Lauf** — jeder Spawn als Transaktion: isolierter Worktree → FS-Delta-Detection → Verifikation → Promote/Reject → **Record**. Auditierbarer, replaybarer, reversibler Envelope um jeden Agent-Effekt. | Power Loom (`shashankcm95/claude-power-loom`) | 🟢 | komponiert mit Merge-Lock (Promote = Merge-Gate-Pass) + A6 | **L** |
| C4 | **Persistente Identität + Reputation** — per-Identität Trust-Score + Verhaltenshistorie auf Disk → rollenbasierte Capability-Injektion statt reiner Agent-Disziplin. | Power Loom | 🟡 (kraftvoll, aber Modell-Frage) | Reputation passt zu hub-Arbitrierung; vorsichtig (Fixed-Fleet hat wenig Reputations-Bedarf) | **M** |
| C5 | **Run/Attempt-Tracking mit Kosten** — jede Task-Ausführung als `Run`-Objekt (Tokens, Kosten, Resultat). | gnap `runs/` (67★) · Gastown beads (*aus Doku*) | 🟡 Provenienz+Budget | Attribut am Task-Entity (E1) | **M** |

### D. Enforced Contracts & Pre-Merge-Gating — Durchsetzung jenseits von Datei-Leases (🟢)

| # | Feature | Quelle | Wert | Fit | Aufwand |
|---|---|---|---|---|---|
| D1 | **Enforced Funktions-Signatur-Contracts** — zwei Agenten einigen sich auf eine Signatur/ein Wire-Format; ein pre-commit-Hook **blockt** Commits, die den vereinbarten Vertrag ändern. CLAUDE.md erlaubt Peer-„Schnittstellen aushandeln" als *einzige* Peer-Kollaboration — heute zahnlose Prosa; dies gibt ihr Zähne. | wit (`amaar-mc/wit`, 45★) | 🟢 | exzellent — vorhandenes tree-sitter snapshottet die Signatur, Verifikation am Merge/Commit-Gate; paart mit Merge-Lock | **M** |
| D2 | **Pre-Merge-Enforcement-Gate als Merge-Lock-Vorbedingung** — automatischer Review (`PR-Agent` o. `claude -p`) **plus** harte Agent-PR-Gates: kein Test-Entfernen/Skip, keine Coverage-Manipulation, keine Permission-Eskalation, kein still aufgeweichtes Gate. Macht Merge-Lock von *Serialisierung* zu *Qualitäts-Gate*. Mappt 1:1 auf CLAUDE.md „tragende Invarianten sind tabu für Shortcuts". | Qodo `pr-agent` (11.9k★) · GitHub-Agent-PR-Playbook · Cloudflare (131k AI-Reviews/30d) | 🟢 | stark — hub fährt Review vor `merge-lock`, blockt bei Findings; manche Regeln = grep/AST auf staged-diff (tree-sitter da) | **M** |
| D3 | **Spekulative cross-branch-Konflikt-Probe** — hub kennt alle Worktrees/Branches; im Hintergrund Paare von in-flight-Branches dry-mergen → früh warnen, *bevor* Merge-Lock umkämpft ist. Natürliche Erweiterung der vorhandenen advisory Call-Graph-Warnung in die *cross-file/cross-branch*-Dimension. | Crystal speculative analysis / DeltaImpactFinder (arXiv 1509.04207) | 🟢 | stark (git dry-merge + build/test auf Union; textuell starten, semantisch später) | **M** |
| D4 | **Semantischer Merge-Konflikt via generierter Tests (SAM)** — Unit-Tests als Teil-Spezifikation jeder Seite generieren, auf dem Merge laufen → *semantische* Konflikte fangen, wo zwei je-korrekte Edits zusammen Verhalten brechen (kein Text-Overlap). | SAM (arXiv 2310.02395, *research-grade*) | 🟢 konzeptionell, schwer | in D2-Gate einsetzen, nicht in Lease-Layer; Test-Gen pro Merge teuer | **L** (exploratorisch) |
| D5 | **Test-Impact-Analyse** — betroffene Test-Teilmenge selektieren (macht D2/D4 bezahlbar); „social signal" (wer/welche Rolle berührt welchen Bereich) = Concords Lease-/Ownership-Daten → hub *prognostiziert Kollisionskurse*. | TIA-Forschung (ResearchGate 319637291; ScienceDirect S0164121224001158) | 🟡 | komponiert mit Leases (Ownership=Signal) + Gate | **M** |

### E. Arbeitsstruktur — die Schicht *über* den Leases (🟢/🟡)

| # | Feature | Quelle | Wert | Fit | Aufwand |
|---|---|---|---|---|---|
| E1 | **Task-Board mit Dependency-DAG + Auto-Unblock** — Work-Items als first-class Objekte (Todo→Doing→Review→Done), `depends_on`-Kanten mit **Zyklus-Erkennung**, **Completion-Cascade unblockt Dependents automatisch**. Gibt hub eine maschinenlesbare **Vision-Kritikpfad**-Sequenzierung (heute nur in hubs Kopf + Prosa); Leases werden aus geclaimten Tasks *abgeleitet*. | agent-kanban (376★) · swarm-protocol (49★) · guild (316★) · gnap (67★) · workgraph / Task Master (27.7k★, dependency-graph-Schema) | 🟢 | komponiert (Task ownt Leases; claim→Lease, complete→release+signal via Push-Daemon) | **L** |
| E2 | **Getrackte Eskalations-Primitive** — Blocker eskalieren mit Severity, geroutet die Kette hoch, **erzeugt ein getracktes Objekt das bis zur Auflösung persistiert** → Blocker können nicht still verschwinden; gibt hub eine echte Queue fürs Forwarding an den Operator. CLAUDE.md-Eskalation ist heute reine Prosa („Worker erreichen den Operator nicht"). | Gastown (16.1k★, *aus Doku*) · Agent-Mail `mark_urgent`/`fetch_urgent_inbox` (2k★) | 🟢 | exzellent (typisierte Nachricht im vorhandenen Inbox-Demux, persistiert mit open/closed-State, in hub-Status sichtbar) | **M** |
| E3 | **Message-Ack/Read-Receipt-Tracking (enforced)** — per-Empfänger `ack_ts`/`read_ts`; `acknowledge_message`. CLAUDE.md *mandatiert* ACK („kein ACK binnen Tick → hub liefert neu/eskaliert") — heute manuelle Prosa-Zeile. Maschinen-Acks lassen Push-Daemon/hub un-ge-ACK-te Direktiven *automatisch* re-deliver/eskalieren. | Agent-Mail (2k★, *aus Doku*) | 🟢 | exzellent — Push-Daemon macht schon Inbox-Demux; Ack-State + TTL-Re-Deliver ergänzen; mechanisiert vorhandene Policy | **M** |
| E4 | **Generische benannte Resource-Locks / Build-Slots (Semaphore)** — advisory Locks auf *Nicht-Datei-Ressourcen* (CI, Deploys, **Ports**) + shared/exclusive mit **N-Slot-Semantik**. ais hat genau diese Contention: **QEMU-Ports**, **build-env**, gegenseitiges QEMU-Killen — heute in Pfad-Leases gezwängt o. Konvention. | claude-presence (7★) · Agent-Mail Build-Slots (2k★) | 🟢 (konkret für ais) | exzellent — Lease-Engine mit `kind=resource`-Namespace + Slot-Count; nutzt Fencing/TTL/Reclaim wieder | **S–M** |
| E5 | **Context-Package / Handoff-Brief (One-Call-Onboarding)** — ein Aufruf liefert vollen Reorientierungs-Stand (aktive Claims, Dependencies, Signale, an-mich-Zuweisungen, offene Eskalationen) + ein „Brief für die nächste Session". Schneidet Re-Orientierungs-Kosten/Drift nach Dormanz scharf. | swarm-protocol `get_context` (49★) · guild `brief` (316★) · Gastown `seance` (*aus Doku*) | 🟢 | komponiert (reines Aggregations-MCP-Tool über Registry/Lease/Inbox + 1 Brief-Record) | **S–M** |
| E6 | **Shared/Exclusive Lease-Modi** — Reservierungen shared (multi-reader) oder exclusive; mehrere Sessions halten Read-Lease auf eine heiße Datei (docs, Header) ohne Serialisierung. | Agent-Mail (2k★) | 🟡 | triviale Erweiterung der Lease-Engine | **S** |
| E7 | **Git-native Task-Cards (Markdown, versioniert)** — selbst-enthaltene Karten (Kontext, Acceptance-Kriterien, Historie) in Git statt im Chat. Stärkt Provenienz-Trail (wer war wofür zuständig, mit welcher Acceptance-Bar). | Backlog.md (`MrLesk/Backlog.md`, 5.85k★) | 🟡 Provenienz-nah | guter Fit (git-native, keine DB); kann ad-hoc-SESSION-SYNC-Einträge ablösen | **S–M** |

### F. Commodity / orthogonal — notieren, nicht (selbst) bauen (🟡/⚪)

| # | Feature | Quelle | Einordnung |
|---|---|---|---|
| F1 | **Statusline-HUD** — id, gehaltene Leases, Merge-Lock-Holder, offene hub-Direktiven pro Session (stdin-JSON ~300ms). | Claude Code Statusline (offiziell) | 🟡 Mechanismus commodity, *Inhalt* (Coord-State) Concord-spezifisch; **S**, billiger Sichtbarkeits-Gewinn |
| F2 | **Threaded, volltext-durchsuchbares Message-Archiv** (FTS5). | Agent-Mail (2k★) | 🟡 navigierbarere Arbitrierung/Audit, aber braucht echten Datastore (Concord-Kanal=Markdown); **M–L** |
| F3 | **Semantisches/hybrides Memory** (BM25+Vektor, RRF, ONNX). | guild (316★) | 🟡 schwerstes Item (Embeddings); orthogonaler Add-on; **L** |
| F4 | **Thread-/Decision-Summarization** — `summarize_thread` destilliert Entscheidungen+Action-Items (→ ADRs). | Agent-Mail (2k★) | 🟡 braucht LLM-Call; **M** |
| F5 | **Checkpoint-recoverable Workflow-Templates** — wiederkehrende Choreografien (z.B. „build-env claim→setup→teardown") als recoverable Templates. | Gastown molecules · hcom `run <workflow>` (360★) | 🟡 Schicht über E1; **M–L** |
| F6 | **Peer-Activity observe/subscribe** — auf Status-Flips/Edits anderer Agenten subscriben + Auto-Notify. | hcom (360★) | 🟡 Edit-Kollision schon via Leases abgedeckt; Event-Subscription paart mit B3-Watchdog; **M** |
| F7 | **Plan-Approval-Gate** — Teammate arbeitet read-only bis Lead approved. | Agent Teams (offiziell) | 🟡 strukturierte Version für riskante Arbeit; via A1-Deny + Lease-Gate spiegelbar |
| F8 | **Kanban-UX / Worktree-per-Worker** (Vibe Kanban 27.2k★, Claude Squad, canopy 101★, muxara). | div. | ⚪ Worktree-**Isolation** umgeht das Problem, das Concord *löst* (geteilter Schreib-Surface) — kein Koordinations-Fortschritt |
| F9 | **Merge-Queue-Produkte** (Graphite/Aviator/Trunk, Gastown Refinery bisecting). | div. | ⚪ Merge-Lock deckt das ab; *einziger* Borrow: „vor Merge gegen frisch gefetchtes origin/main re-validieren" (S) |
| F10 | **Cross-Device/Federation** — MQTT-Relay (hcom), DoltHub-Federation + portable Reputation (Gastown Wasteland), Cross-Project-Contact-Handshake (Agent-Mail). | hcom/Gastown/Agent-Mail | ⚪ orthogonal — Concord ist ein Host, fixe Fleet |
| F11 | **Agent SDK / `canUseTool` / in-process MCP** — Hooks programmatisch, falls Concords Launcher Sessions selbst treibt. | Claude Agent SDK (offiziell) | 🟡 größeres Architektur-Commitment; „Hooks=Invarianten, `canUseTool`=session-spez. Policy" mappt auf Concords enforced-vs-advisory-Split; **L** |
| F12 | **Heavyweight-Observability-Plattformen** (Langfuse 30k★, AgentOps 5.7k★, Helicone). | div. | ⚪ als Dependency off-vision; *Insight* (Session-Level-Loop/Drift) nativ auf B1 bauen; Langfuse=OTLP-Fallback-Store |

> **Interaktions-Hazard (kein Feature, aber behandeln):** Claude Codes **Checkpointing/`/rewind`** (v2.0+) ist
> per-Session+lokal und trackt **keine** bash-modifizierten Dateien (`rm`/`mv`) — der Großteil von ais' Build/Test.
> Eine Session, die rewindet *während sie ein Lease hält*, desynct Concords Sicht. Concord sollte einen
> `SessionStart`-Reconcile-Guard vorsehen. Quelle: <https://code.claude.com/docs/en/checkpointing>.

---

## 3. Priorisierte Empfehlung

### ADOPTIEREN — hoher Wert, vision-treu, guter Fit, vertretbarer Aufwand

| Rang | Kandidat | Warum jetzt | Aufwand |
|---|---|---|---|
| **1** | **A1–A5: Harness-native Enforcement-Verdrahtung** (`PreToolUse`+`mcp_tool`-Deny, `SessionEnd`-Release, `Stop`-anti-dark, `PreCompact`-Survival, `FileChanged`-Wake) | **Billigster, vision-treuester Hebel:** macht Leases *hart* statt advisory und kuriert „going dark" harness-nativ. Infrastruktur existiert, nur Verdrahtung fehlt. Hebt Concords Kern-Invariante. | S–M je Hook |
| **2** | **E4: Benannte Resource-Locks / Build-Slots (Semaphore)** | Löst die *dokumentierte* ais-Contention (QEMU-Ports, build-env) sauber; reine Erweiterung der Lease-Engine. | S–M |
| **3** | **E3 + E2: Ack-Tracking + getrackte Eskalation** | Mechanisiert zwei CLAUDE.md-Policies (ACK-Protokoll, Blocker-Eskalation) die heute zahnlose Prosa sind; nutzt Push-Daemon-Demux. | M |
| **4** | **B1 (+B2): hub-Telemetrie auf nativem OTel + ccusage** | Macht hub *telemetrie-getrieben* (Burn/Idle/Loop/Drift messbar); setzt „kein stilles Idlen" durch. Emittierende Seite ist gratis/built-in. | M (S für B2) |
| **5** | **D1: Enforced Signatur-Contracts** | Gibt der einzigen erlaubten Peer-Kollaboration (Interface-Aushandeln) Zähne; nutzt vorhandenes tree-sitter. | M |
| **6** | **A6 + B3: Out-of-scope-Write-Detection + Dark-Session-Watchdog** | Audit-Zähne hinter den Leases + aktives Alerting statt passivem Reclaim — direkt gegen den #1-Failure-Mode. | M |

### BACKLOG — wertvoll, aber größer / abhängig / später

- **D2 Pre-Merge-Enforcement-Gate** (M) — Merge-Lock zu Qualitäts-Gate; nach ADOPTIEREN-Welle.
- **D3 spekulative cross-branch-Konflikt-Probe** (M) — proaktiver hub; baut auf Call-Graph-Warnung auf.
- **C1 kryptografische Identität + signierte Commits** (M–L) — stärkste Provenienz-Play; größeres Commit-Signing-Stück.
- **C2 governed provenance-stamped Decision-Store** (M–L) — `coord.sh log` zur enforced, fencing-versionierten Vertikale ausbauen.
- **E1 Task-DAG + Auto-Unblock** (L) + **E5 Context-Brief** (S–M, billiger Begleiter) — größte strukturelle Ergänzung, gibt hub einen echten Kritikpfad.
- **E5/E7/F1** als billige Sichtbarkeits-/Provenienz-Gewinne (S–M) jederzeit einstreubar.

### MUSTER ÜBERNEHMEN (nicht das Produkt)
- workgraph/saltbo **Dependency-Frontier-Work-Stealing** für hub-Dispatch; **Task Master**s Dependency-Schema; **Backlog.md** git-native Cards; Power Looms **transaktionaler Envelope** (C3) als Fernziel.

### VERWERFEN / NUR NOTIEREN
- **F8 Kanban/Worktree-Isolation** (umgeht Concords Problem), **F9 Merge-Queue-Produkte** (Merge-Lock genügt), **F10 Cross-Device/Federation** (Single-Host), **F12 Heavyweight-Observability als Dependency** (Insight nativ bauen). **D4/D5 (SAM/TIA)** und **F2/F3 (FTS/Vektor-Memory)** nur als Spike, wenn ein konkretes Problem sie zieht — sonst Over-Engineering.

---

## 4. Unsicherheiten / Quellenlage

- Star-/Push-Daten Stand 2026-06-29 via `gh`/WebFetch; sehr junge Repos volatil.
- Interne Feld-/Tool-Details mehrerer Peers (Agent-Mail `ack_ts`/`message_recipients`, agent-kanban Ed25519,
  Gastown Watchdog-Tiers) sind **aus README/Doku-Prosa abgeleitet, nicht aus Quellcode verifiziert** — als
  illustrativ behandeln; vor Code-Übernahme prüfen (Agent-Mail-Lizenz = „Other"/non-standard).
- **A6/C2/C3/D3/D4/D5** sind teils Forschung/frühe Tools, **nicht** drop-in — als *nativ zu implementierende
  Muster* gemeint, was sie zugleich in Concords governte, local-first-Vision hält.
- `mcp_tool`-Hook (A1): MCP verbindet **nach** `SessionStart` — Enforcement-Hook muss auf `PreToolUse` liegen.

## Quellen (Auswahl)
- Claude Code Hooks (alle Events): <https://code.claude.com/docs/en/hooks> · Agent Teams: <https://code.claude.com/docs/en/agent-teams> · Statusline: <https://code.claude.com/docs/en/statusline> · OTel/Observability: <https://code.claude.com/docs/en/agent-sdk/observability> · Checkpointing: <https://code.claude.com/docs/en/checkpointing>
- Plan-Mode-nicht-enforced: <https://blog.sondera.ai/p/claude-codes-plan-mode-isnt-read>
- Peers: Agent Mail <https://github.com/Dicklesworthstone/mcp_agent_mail> · Gastown <https://github.com/gastownhall/gastown> · wit <https://github.com/amaar-mc/wit> · agent-kanban <https://github.com/saltbo/agent-kanban> · guild <https://github.com/mathomhaus/guild> · gnap <https://github.com/farol-team/gnap> · swarm-protocol <https://github.com/phuryn/swarm-protocol> · claude-presence <https://github.com/garniergeorges/claude-presence> · hcom <https://github.com/aannoo/hcom> · Power Loom <https://github.com/shashankcm95/claude-power-loom> · Worksidian <https://github.com/StefanOjanen/worksidian>
- Observability: ccusage <https://github.com/ryoppippi/ccusage> · Usage-Monitor <https://github.com/Maciek-roboblog/Claude-Code-Usage-Monitor> · claude-trace (`badlogic/lemmy`) · agent-exporter <https://github.com/chaspy/agent-exporter> · disler multi-agent-observability <https://github.com/disler/claude-code-hooks-multi-agent-observability>
- Gating/Boards: Qodo pr-agent <https://github.com/qodo-ai/pr-agent> · GitHub Agent-PR-Playbook <https://github.blog/ai-and-ml/generative-ai/agent-pull-requests-are-everywhere-heres-how-to-review-them/> · Task Master <https://github.com/eyaltoledano/claude-task-master> · Backlog.md <https://github.com/MrLesk/Backlog.md> · Vibe Kanban <https://github.com/BloopAI/vibe-kanban> · workgraph <https://graphwork.github.io/>
- Research: Governed Memory arXiv 2606.24535 / 2505.18279 · SAM arXiv 2310.02395 · DeltaImpactFinder arXiv 1509.04207
