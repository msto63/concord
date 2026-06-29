# Concord — Competitive Landscape & Prior-Art Research

> Stand: 2026-06-29. Recherche via GitHub (`gh search/api`) + Web (WebSearch/WebFetch).
> Frage: Gibt es bereits Tools, die dasselbe tun wie Concord? Wo ist Concords Alleinstellung?

## 0. Was Concord ist (das Vergleichsraster)

Concord koordiniert **mehrere unabhängige KI-Coding-Agenten** (parallele Claude-Code-Terminal-Sessions),
die **gleichzeitig am selben Git-Repo** arbeiten, über ein **Dateisystem-Substrat** — kein In-Process-Framework.
Sechs distinktive Fähigkeiten dienen als Achsen:

1. **Multi-Session-Registry** (wer aktiv, Fokus, Heartbeat + TTL-Stale-Reclaim)
2. **Area-Leases** — wechselseitiger Ausschluss auf Pfaden/Code-Regionen (claim/release, Stale-Reclaim, Pfad-Präfix-Overlap)
3. **Singleton Merge-Lock** — nur eine Session merged zur Zeit
4. **Inter-Agent-Kommunikation** (Prosa + strukturierte `→`-Direktiven; Koordinator-Agent „hub")
5. **Hooks-Integration mit Claude Code** (auto-register, Direktiven-Injektion, Lease-Guard auf Tool-Calls)
6. Roadmap: typisierter **Rust-Kern + Push-Daemon (fs-watch)** + **Fencing-Tokens (Kleppmann)** + **MCP-Server**

**Entscheidende Trennlinie:** „Worktree-Multiplexer ohne Koordination" (nur parallele Isolation, jeder Agent
im eigenen Worktree, kein gemeinsamer Schreib-Surface) vs. **echte Koordination** (Leases / Merge-Lock /
Inter-Agent-Comms auf einem *geteilten* Repo). Concord ist zweiteres.

---

## 1. Direkte Äquivalente — „echte Koordination" (gleiche Kategorie wie Concord)

### 🥇 MCP Agent Mail — `Dicklesworthstone/mcp_agent_mail` — **2.011★**, aktiv (Push 2026-06-27)
Rust (12-Crate-Workspace, Tokio) + FastMCP + Git + SQLite. Python-Vorgänger hatte 1.700★+ vor dem Rust-Rewrite.
> „Asynchronous coordination layer for AI coding agents: identities, inboxes, searchable threads, and advisory file leases over FastMCP + Git + SQLite". Webseite: https://mcpagentmail.com/

**Das ist Concords Feature-Set — fast 1:1, reifer und als MCP-Produkt verpackt:**
- **Advisory File-Leases** auf **Globs** (`src/auth/**/*.ts`), TTL (default 3600s), **automatische Stale-Reclamation**, **Pfad-Präfix-Overlap-Detection** built-in, exclusive *und* shared Leases.
- **Pre-commit Git-Hook** `mcp-agent-mail-guard`, der Commits auf von anderen reservierte Dateien **blockiert** (bypassbar via `AGENT_MAIL_BYPASS=1`).
- **Build-Slots** — explizite Begrenzung gleichzeitiger Builds (≈ Concords Merge-/Singleton-Lock-Idee, generalisiert auf Ressourcen).
- **Inter-Agent-Messaging** — threaded, Recipients/CC/BCC, Importance, Ack-Tracking, im Git-Archiv (kein Kontext-Verbrauch).
- **Agent-Registry** — semi-persistente Identitäten (GreenCastle/BlueLake), projekt-scoped, Programm/Modell-Metadaten, TTL-Expiry für verwaiste Agenten.
- **Human-Overseer** — Web-UI + TUI-Dashboard, High-Prio-Nachrichten mitten in der Session (≈ Concords „hub"-Rolle, aber als Mensch/Dashboard statt Agent).
- 34 MCP-Tools, getestet mit 40–50 nebenläufigen Agenten.

| Concord-Fähigkeit | Agent Mail |
|---|---|
| Multi-Session-Registry | ✓ (mit TTL) |
| Area-Leases (+overlap, +stale) | ✓ (Globs, TTL, overlap, stale-reclaim) |
| Merge-/Singleton-Lock | ✓ teilweise (Build-Slots, generische Ressourcen-Locks) |
| Inter-Agent-Comms | ✓ (reicher: threads, ack, search) |
| Hooks/Claude-Code-Integration | ✓ (pre-commit-Guard; CC/Codex/Gemini) |
| Koordinator-Rolle | ✓ teilweise (Human-Overseer-Dashboard, kein autonomer „hub"-Agent) |

**Unterschied zu Concord:** MCP statt Shell/CLI; Mensch-Overseer statt autonomer Koordinator-Agent „hub"; Messaging
zentriert (Name „Mail"), Concord hat eine explizite **arbitrierende Koordinator-Session am Vision-Kritikpfad**.
Funktional ist Agent Mail **das nächste Äquivalent** und in jeder Mechanik (Leases/overlap/stale/hook) mindestens so weit.

### claude-presence — `garniergeorges/claude-presence` — 7★, Push 2026-05-22
> „Minimal MCP server for inter-session coordination between parallel Claude Code instances. Presence registry + advisory resource locks (CI, deploys, ports) + broadcast inbox. SQLite-backed, zero daemon."
Konzeptionell **sehr nah an Concord**, aber minimal: Registry ✓, advisory Locks ✓ (auf benannte Ressourcen, nicht primär Pfad-Globs/Overlap), Broadcast-Inbox ✓. Kein Merge-Lock-Singleton, kein autonomer Koordinator, keine Pfad-Präfix-Overlap-Logik. 9 MCP-Tools, auto-Branch/CWD-Detection. Zielgruppe identisch (parallele Claude-Code-Sessions, Koordination via CLAUDE.md-Instruktion).

### swarm-protocol — `phuryn/swarm-protocol` — 49★, Push 2026-03-15
> „Headless coordination layer exposed as MCP server: claim work, detect file conflicts, heartbeat, and hand off tasks across agent sessions." — Registry/heartbeat ✓, File-Conflict-Detection ✓ (≈Leases), Task-Handoff ✓, Comms (state sync) ✓. Kein expliziter Merge-Lock; kein Koordinator-Agent („No UI, no Jira, just state sync"). Direkter Kategorie-Peer.

### wit — `amaar-mc/wit` — 45★, Push 2026-03-27
> „Agent coordination protocol — declare intents, lock symbols, detect conflicts before code is written." **Symbol-/Funktions-Level-Locks via Tree-sitter-AST** — *granularer* als Concords Pfad-Leases. Intents + Konflikt-Warnungen vor dem Schreiben. Kein Merge-Lock/Registry-Schwerpunkt. **Lernkandidat** für Concord (AST-Granularität schlägt Datei-Granularität bei „zwei Agenten, dieselbe Datei, andere Funktionen").

### guild — `mathomhaus/guild` — 316★, Push 2026-06-22
> „Shared context, memory, and task coordination across AI coding agents. Single Go binary, local SQLite, hybrid keyword + semantic search." Registry/Task-Coord ✓, geteilter Speicher/Comms ✓ (semantische Suche — über Concord hinaus). Schwächer bei harten Leases/Merge-Lock. Architektur-Vorbild (Single-Binary + SQLite ≈ Concords Rust-Kern-Roadmap).

### hcom — `aannoo/hcom` — 360★, Push 2026-06-29 (sehr aktiv)
> „Hook your AI coding agents together so they can message, watch, and spawn each other across terminals." **Hook-basierte Inter-Agent-Comms + Spawn** über Terminals (CC, Codex, Cursor, OpenCode, Kimi …). Deckt Concords Achse 4+5 (Comms+Hooks) sehr direkt ab; **kein** Lease-/Merge-Lock-Schwerpunkt. Bestätigt Concords Hook-Injektions-Design als gängiges Muster.

### gnap — `farol-team/gnap` — 67★, Push 2026-03-17
> „Git-Native Agent Protocol — RFC Draft for git-based agent orchestration. Zero servers." Geteiltes Git-Repo als **persistenter Task-Board (todo/doing/done)**, **kein Orchestrator-Prozess**. Philosophie wie Concord (Dateisystem/Git als Substrat, keine zentrale Instanz), aber Task-Board-zentriert, keine Datei-Leases/Merge-Lock. **Relevant**: als formalisiertes RFC ein Standardisierungs-Vorbild.

### Weitere Peers (Kanban/Comms-lastig, Leases schwächer)
- **shire** `victor36max/shire` (34★) — persistente Workspaces, **Inter-Agent-Mailboxes + Shared Drive**, Kontext-Erhalt.
- **agent-kanban** `saltbo/agent-kanban` (376★) — Leader-Worker-Modell, **kryptografische Agent-Identität**, Multi-Runtime. Leader-Worker ≈ Concords hub→Sessions.
- **multi-agent-coordination-mcp** `AndrewDavidRivers/...` (7★), **clawe**, **CompanyHelm**, **ORCH** `oxgeneral/ORCH` (typed teams + state machine), **gastown** (Steve Yegge, persistent work tracking) — alle mit Teilmengen (Comms/Task-Coord), keiner mit Concords vollem Lease+Merge-Lock+Koordinator-Triplet.

---

## 2. Worktree-Multiplexer **ohne** echte Koordination (andere Kategorie — nur parallele Isolation)

Diese lösen Kollisionen durch **Isolation** (jeder Agent eigener Worktree/Container/Pod), nicht durch
geteilte Leases/Merge-Lock auf *einem* Schreib-Surface. Sie sind **nicht** das, was Concord tut — aber sie sind die
populärste Antwort auf „mehrere Agenten parallel". Mensch reviewt + merged manuell (per-edit-approval).

| Tool | Repo | ★ | Aktualität | Mechanik |
|---|---|---|---|---|
| **Vibe Kanban** | `BloopAI/vibe-kanban` | **27.203** | 2026-04-24 | Kanban über CC/Codex, Worktrees |
| **Claude Squad** | `smtg-ai/claude-squad` | **7.957** | 2026-06-17 | tmux + Worktree pro Agent (CC/Codex/Aider/Gemini) |
| **agent-orchestrator** | `AgentWrapper/agent-orchestrator` | 7.757 | 2026-06-29 | parallele Agenten |
| **container-use** | `dagger/container-use` | 3.894 | 2026-06-12 | je Agent eigene Dev-Umgebung (Dagger) |
| **Crystal → Nimbalyst** | `stravu/crystal` | 3.093 | 2026-02-26 (deprecated) | Desktop, parallele Worktrees |
| **multi-agent-shogun** | `yohey-w/multi-agent-shogun` | 1.360 | 2026-06-06 | tmux shogun→karo→ashigaru, „zero coordination API cost" |
| **uzi** | `devflowinc/uzi` | 579 | 2025-06-04 | CLI, viele Agenten in Worktrees |
| **sculptor** (imbue) | `imbue-ai/sculptor` | 188 | 2026-06-29 | parallele Agenten |
| **amux** | `andyrewlee/amux` | 132 | 2026-06-25 | TUI, parallele Agenten |
| **Conductor** (Melty) | (Mac-App, closed) | — | 2026 | Worktree + Diff/PR-Flow |

Plus dutzende weitere im *awesome-agent-orchestrators*-Index (parallel-code, dmux, agentbox, agenttier, clave,
constellagent, herdr, tutti, …). **Kernpunkt:** Diese garantieren *Konfliktfreiheit durch Trennung*, nicht durch
Koordination. Wenn Agenten denselben Worktree teilen sollen (Concords Szenario), helfen sie nicht.

---

## 3. In-Process-Multi-Agent-Frameworks (**andere Kategorie** — nur Abgrenzung)

Diese orchestrieren Agenten **in einem Prozess/einer Session** (ein Lead spawnt Worker im selben Kontext-/Tool-Raum),
**nicht** unabhängige Betriebssystem-Sessions auf einem geteilten Repo. Kein Dateisystem-Substrat, kein Stale-Reclaim
abgestürzter Peers, keine cross-Session-Leases — die ganze Problemklasse, die Concord adressiert, existiert hier
gar nicht (der Orchestrator hat ohnehin In-Memory-Hoheit über alle Worker).

- **Anthropic Agent Teams** (offiziell, experimentell, `CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS`, v2.1.178+) — **der wichtigste offizielle Datenpunkt.** Ein Lead spawnt Teammates (eigene Kontextfenster), **Shared Task-List + Mailbox**, direkte Teammate-zu-Teammate-Messages, `TaskClaim` per **File-Locking** gegen Races, Hooks (`TeammateIdle/TaskCreated/TaskCompleted`). **ABER:** *„one team per session"*, kein Worktree, **keine Area-Leases** (Doku rät ausdrücklich: „Two teammates editing the same file leads to overwrites. Break the work so each teammate owns a different set of files."), kein Merge-Lock, Lead fix, keine Cross-Session-/Cross-Maschinen-Koordination. → Es löst *Aufgabenverteilung in einer Session*, **nicht** *Koordination separater Sessions am geteilten Repo*. Quelle: https://code.claude.com/docs/en/agent-teams
- **Claude Code Subagents** — Helfer *innerhalb* einer Session, melden nur an Main zurück, reden nicht miteinander. Noch weiter weg.
- **AutoGen / CrewAI / LangGraph / OpenAI Swarm / claude-swarm** — In-Process-Conversation-/Graph-Orchestrierung. Komplett andere Kategorie.

---

## 4. Distributed-Locking-Primitive (nur **Pattern-Quelle**, kein Agenten-Tool)

- **etcd / Consul / ZooKeeper** — Infrastruktur-Locks/Leases mit Lease-TTL + Fencing. **Kein** Agenten-Tool, aber das *Pattern*-Vorbild für Concords Rust-Kern.
- **Martin Kleppmann, „How to do distributed locking" (2016)** — Fencing-Tokens (monoton steigende Token, Storage lehnt veraltete ab). Concords geplante Fencing-Token-Schicht ist genau das; Pflichtlektüre und korrekt zitiert in der Roadmap. Quelle: https://martin.kleppmann.com/2016/02/08/how-to-do-distributed-locking.html

---

## 5. Fazit (die vier Pflicht-Fragen)

**1. Gibt es ein DIREKTES Äquivalent zu Concord (gleiche Kern-Fähigkeiten)?**
**Ja — `Dicklesworthstone/mcp_agent_mail` (MCP Agent Mail, 2.011★).** Es deckt 5 von 6 Concord-Achsen direkt und
reifer ab: Datei-Leases mit Glob/TTL/**Stale-Reclaim/Pfad-Präfix-Overlap**, Build-Slots (≈Merge-Lock), Inter-Agent-
Messaging, Agent-Registry mit TTL, **pre-commit-Hook-Guard**, Human-Overseer-Dashboard — und ist bereits der von Concord
als Roadmap angepeilte **Rust+SQLite+MCP**-Endzustand. Concords Pfad-Präfix-Overlap-Erkennung, die als „neu" geführt
wird, ist dort **bereits Standard.** Daneben mehrere kleinere echte Peers (claude-presence, swarm-protocol, wit, hcom,
guild, gnap). **Concords Mechanik ist nicht neu** — sie ist 2026 ein etablierter Tool-Typ.

**2. Concords Alleinstellung — und wo redundant?**
*Schmal, aber existent:* (a) ein **autonomer Koordinator-*Agent* „hub"**, der am *Vision-Kritikpfad* sequenziert,
Ownership arbitriert und Merges neutral reiht — die meisten Tools haben entweder *keinen* Koordinator (peer/state-sync:
swarm-protocol, gnap) oder einen *menschlichen* Overseer (Agent Mail). (b) Die **enge Verzahnung mit einer
Projekt-Governance-/Vision-Leitplanke** (CLAUDE.md-Protokoll, „Vision schlägt Mainstream"). *Redundant/überholt:* die
**reine Locking-/Lease-/Registry-/Comms-Mechanik** — hier ist Concord gegenüber Agent Mail eher hinterher (Shell-CLI vs.
getesteter Rust-MCP mit Web/TUI-Dashboard, Threads, Ack, Search). Die Shell-CLI-Form ist Concords schwächster Punkt.

**3. Was kann Concord lernen/übernehmen?**
- **Symbol-/AST-Level-Leases (wit)** statt nur Datei-Pfade — löst „zwei Agenten, dieselbe Datei".
- **Glob-Leases + bypassbarer pre-commit-Guard (Agent Mail)** — Hook am *Commit* zusätzlich zum Tool-Call-Guard.
- **MCP-Server-Schnittstelle (Agent Mail, claude-presence, swarm-protocol)** — Tools statt Prosa-Parsing; Concord plant das bereits, sollte es priorisieren.
- **Threaded Messaging mit Ack-Tracking + Such-Index (Agent Mail/guild)** statt einer einzigen Prosa-Datei (SESSION-SYNC.md skaliert nicht).
- **gnap als RFC** — Standardisierung des Substrat-Formats macht das Protokoll tool-übergreifend.
- **Single-Go-Binary/SQLite (guild)** als pragmatischere Zwischenstufe vor dem vollen Rust-Daemon.

**4. Ehrliche Einordnung — lohnt sich Concord als eigenständiges Tool?**
**Als generisches Produkt: eher nein.** Für „koordiniere parallele Coding-Agenten am geteilten Repo per Datei-Leases +
Comms" ist **MCP Agent Mail** heute die überlegene, reifere, breiter getestete Wahl (Rust-MCP, Dashboard, 2k★), und für
Minimal-Setups gibt es claude-presence/swarm-protocol. Concords Lease-/Merge-Lock-/Registry-Kern erfindet einen
2026 bereits gelösten Standard nach. **Als projekt-internes Werkzeug mit spezifischem Mehrwert: ja, bedingt** — der
distinkte Teil ist *nicht* das Locking, sondern der **autonome, vision-getriebene Koordinator-Agent + Governance-Leitplanke**.
Empfehlung: **Mechanik (Leases/Registry/Comms) nicht selbst bauen, sondern auf Agent Mail oder einen MCP-Server
aufsetzen** und Concords Energie auf das wirklich Eigene konzentrieren — die hub-Arbitrierung am Vision-Kritikpfad und
die durchgesetzte Governance. Andernfalls baut Concord eine schlechtere Kopie eines existierenden 2k★-Tools.

*Unsicherheiten:* Stars/Push-Daten Stand 2026-06-29; einige Tools sehr jung/volatil. Interne Mechanik mancher Repos
(swarm-protocol, wit, guild) nur aus Beschreibung/awesome-Index abgeleitet, nicht aus dem Code verifiziert — bei
strategischen Entscheidungen Agent Mail + wit + claude-presence vor Adoption im Quellcode prüfen.

---

## Quellen
- MCP Agent Mail: https://github.com/Dicklesworthstone/mcp_agent_mail · https://mcpagentmail.com/
- claude-presence: https://github.com/garniergeorges/claude-presence
- swarm-protocol: https://github.com/phuryn/swarm-protocol · wit: https://github.com/amaar-mc/wit · hcom: https://github.com/aannoo/hcom · guild: https://github.com/mathomhaus/guild · gnap: https://github.com/farol-team/gnap · shire: https://github.com/victor36max/shire · agent-kanban: https://github.com/saltbo/agent-kanban
- Anthropic Agent Teams: https://code.claude.com/docs/en/agent-teams · Worktrees: https://code.claude.com/docs/en/worktrees
- awesome-agent-orchestrators: https://github.com/andyrewlee/awesome-agent-orchestrators
- Claude Squad: https://github.com/smtg-ai/claude-squad · Vibe Kanban: https://github.com/BloopAI/vibe-kanban · Crystal/Nimbalyst: https://github.com/stravu/crystal · container-use: https://github.com/dagger/container-use · uzi: https://github.com/devflowinc/uzi
- "Coordinate Multiple Claude Code Sessions on a Shared Repo": https://dev.to/sahil_kat/coordinate-multiple-claude-code-sessions-on-a-shared-repo-1dh4
- Kleppmann, "How to do distributed locking": https://martin.kleppmann.com/2016/02/08/how-to-do-distributed-locking.html
- "9 Open-Source Agent Orchestrators" (Augment): https://www.augmentcode.com/tools/open-source-agent-orchestrators
- "Best Tools for Managing Parallel AI Coding Agents 2026" (Nimbalyst): https://nimbalyst.com/blog/best-agent-management-tools-2026/
