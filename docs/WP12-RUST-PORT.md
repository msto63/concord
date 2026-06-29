# WP12 — Rust-Port von Concord: Design & Stufenplan

> **Status:** Entscheidung offen (Owner: Operator). Dieses Dokument ist der *Plan*, nicht die
> Freigabe — es macht WP12 (BACKLOG) konkret genug, um zu entscheiden und ggf. zu starten.
> Es schließt an ROADMAP §11 an und respektiert dessen Leitplanken (drop-in, koexistierend,
> On-Disk-Layout unverändert). Verfasst von Session `hub` (Koordinator/Steward).

## 1. Zweck & Entscheidungsrahmen

**Frage (ROADMAP §11):** Soll Concord ein einzelnes plattformunabhängiges Rust-Binary werden statt
Shell-Skripten? **Langfristig: ja — aber fähigkeits-getrieben, nicht als 1:1-Rewrite.**

Heute ist Concord ~320 Zeilen Shell (`bin/coord.sh` 139, `bin/concord` 135, 2 scripts) über einem
Dateisystem-Zustand + Claude-Code-Hooks. Das war der **richtige Weg zum Prototyp** — in `ais` geboren,
hat es das Modell schnell bewiesen. Ein reiner Nachbau in Rust wäre wenig wert. Wertvoll wird der Port,
weil er drei *strukturelle* Schwächen behebt, die in Shell nicht sauber lösbar sind:

1. **Der Koordinator ist selbst nicht race-frei.** Ironie eines Anti-Race-Werkzeugs: nicht-atomare
   Lese-Ändere-Schreibe-Zyklen auf Dateien (nur das Lease-`mkdir` ist atomar). Rust erzwingt atomare,
   typisierte Zustandsübergänge.
2. **Polling statt Push** — der dominante Token-Kostentreiber (siehe §8). Ein Daemon liefert Ereignisse
   im Moment des Auftretens statt alle ~12 min „nachschauen".
3. **Plattform-Fragilität** (BSD-vs-GNU: `date -r`/`stat -f`). Rusts stdlib ist portabel → **supersedet
   WP11** (Linux nativ + Windows ohne WSL2) und liefert *ein* installierbares Artefakt.

Bonus: Concord ist philosophisch dasselbe wie ais (durchgesetzte Koordination, Leases≈Capabilities,
Rechenschaft/Provenienz). Sauber in Rust gebaut ist es ein **Dogfood** der ais-Vision auf Werkzeugebene;
Nordstern: Concord als ais-nativer Dienst (§4 M6).

## 2. Leitplanken / Invarianten (nicht verhandelbar)

- **Drop-in & koexistierend.** Das Rust-`concord` muss die Shell-Version Verb-für-Verb ersetzen können
  und **dasselbe On-Disk-Layout** lesen/schreiben, damit beide während der Migration nebeneinander laufen.
- **Dateisystem-als-Wahrheit bleibt inspizierbar.** Der Zustand muss mit `ls`/`cat` lesbar bleiben (kein
  opakes Binärformat als *einzige* Wahrheit). Das ist ein Feature: kein Daemon zu babysitten, überlebt
  Crashes, debugbar. Der Daemon (M2) ist ein *Beschleuniger über* dem FS-Zustand, nicht dessen Ersatz.
- **Parität zuerst, Features danach.** Erst exakte Verhaltensgleichheit (M1), dann neue Fähigkeiten.
  Keine Big-Bang-Neuerfindung.
- **Vision schlägt Bequemlichkeit.** Wo der Standard-Weg eine durchgesetzte Garantie aufweichen würde
  (z. B. Lease-Eigentum nur „per Konvention"), den harten erzwungenen Weg gehen.

## 3. Was portiert wird (die heutige Wahrheit)

**CLI-Verben (`coord.sh`):** `register · heartbeat · status · claim · release · merge-lock ·
merge-unlock · log · sync`.
**Launcher (`concord`):** Fleet-Start, Worktree-Setup, Kickoff-Prompts.
**Hooks:** `session-start · user-prompt · pre-tool · post-tool · statusline · install/uninstall` +
`lib.sh`, `shared-regions`.
**Zustand:** `sessions/<id>` (kv: focus/started/heartbeat), `leases/<area>/` (Verzeichnis = Schloss,
`holder`/`since`/`why`), `intents.jsonl` (Log), Prosa-Kanal `*-SESSION-SYNC.md`.

## 4. Ziel-Architektur — gestuft

> Jede Stufe ist eigenständig wertvoll und einzeln auslieferbar. Stop-Punkte sind bewusst gesetzt:
> man kann nach M1 oder M2 aufhören und hat trotzdem gewonnen.

### M1 — `concord` CLI in Rust, Paritäts-Drop-in  `[Fundament]`
- Workspace-Crate `concord` (bin) + `concord-core` (lib: Zustandsmodell, Übergänge).
- Alle 9 Verben + Launcher + Hooks als **Subcommands** (`concord hook session-start`, `concord coord claim …`).
- **Gleiches On-Disk-Format**, aber Übergänge atomar (flock / atomic-rename) und über einen **typisierten
  Zustandsautomaten** validiert: keine Freigabe/merge-unlock fremden Eigentums; Claim-Konflikt strukturell
  abgelehnt; „Lease ohne Release bei Exit" erkennbar.
- Version aus `Cargo.toml` (`env!("CARGO_PKG_VERSION")`).
- **Akzeptanz:** Shell- und Rust-Version produzieren bit-gleichen Zustand für dieselbe Befehlsfolge
  (Differential-Test-Harness); bestehende Hooks rufen wahlweise Shell oder Binary.

### M2 — `concordd` Daemon: Push statt Poll  `[der Token-Hebel]`
- Optionaler Hintergrunddienst, der den FS-Zustand + Prosa-Kanal beobachtet (`notify`-Crate / fs-events)
  und einen **Notify-Stream** über Unix-Socket (oder Datei-Touch-Fallback) exponiert.
- Hooks/Self-Ticks **subscriben** statt zu pollen → Sessions wachen bei *echten* Ereignissen auf, nicht
  auf Timer. Self-Tick wird zum reinen Zuverlässigkeits-Fallback (langes Intervall) statt Primär-Wecker.
- **Wichtige Ehrlichkeit:** Der Daemon kann eine Claude-Session nicht *zwingen* aufzuwachen (das bleibt
  am Harness) — aber er stellt das Ereignis sofort zu, statt mit bis zu 12 min Verzug, sobald der
  Harness-Watcher subscribed ist.
- **Akzeptanz:** messbarer Rückgang der Idle-Ticks (§8) bei gleicher Reaktionslatenz < 5 s.

### M3 — Typisiertes Board, MCP-Tools, Inbox  `[Roadmap §6/§7/§9 sauber]`
- **Strukturiertes Board** (§6): Merge-Queue, Lease-Graph, Blockade-/Abhängigkeitssicht als echte
  Datenstruktur statt grep über Prosa.
- **Typisierte MCP-Tools** (§7): `concord` exponiert Koordinations-Operationen als MCP-Server → Agenten
  rufen `claim`/`status`/`merge-lock` als Tools mit Schema statt Shell-Strings.
- **Inbox-Protokoll** (§9): kompakte gerichtete Direktiven (`→ <id>`) als typisierte Queue; der
  Prosa-Kanal bleibt der *menschliche* Audit-/Diskussions-Log (§11-Prinzip „keep a human log").

### M4 — Cross-Platform + Distribution  `[supersedet WP11]`
- Native macOS/Linux/Windows (keine BSD-vs-GNU-Hacks, kein WSL2-Zwang).
- `cargo install` + prebuilt Binaries (CI-Release-Matrix), `.gitattributes` LF, Support-Matrix in Doku.

### M5 — Multi-Projekt + Dogfooding  `[Roadmap]`
- Koordinations-Dir + Sync-Pfad pro Projekt-Root ableiten (heute hartkodiert auf ais) → `CONCORD_DIR`/
  `CONCORD_SYNC`-Konfig als typisierte Einstellung. Ein Binary koordiniert mehrere Repos.
- Dogfood: `concord-coord/` + `concord-SESSION-SYNC.md` + Worktrees `concord-a…` → Concords eigene
  Entwicklung mit Concord koordiniert.

### M6 — Concord-auf-ais (Nordstern, post-1.0)  `[Vision-Dogfood]`
- Concord als ais-nativer Dienst (Leases als echte Capabilities, Ledger über `dbd`, Audit über `auditd`).
  Langfristig, nach breiter ais-Reife — hier nur als Richtungsmarke.

## 5. Datenmodell (typisiert, M1+)

```
Session   { id, focus, started: Instant, heartbeat: Instant, state: Active|Stale|Paused }
Lease     { area: Path|Region, holder: SessionId, since: Instant, why: String }
MergeLock { holder: Option<SessionId>, since, note }        // Singleton
LedgerEntry { ts, session, kind: Decision|Intent|Arbitration, body }   // append-only, intents.jsonl
ProseRef  { path }                                          // *-SESSION-SYNC.md, menschlicher Kanal
```

**Per-Session-Konfiguration (rollen-agnostisch, aber rollen-fähig):** Eine Session trägt optional
`charter` (durabler System-Prompt = WER sie ist: „Architekt"/„Tester"/„Generalist auf Terrain Y" —
**vom Nutzer gewählt, nicht vom Tool erzwungen**) + `mission` (per-Task-Auftragsprompt = WAS sie jetzt
tut), beide beim Launch mit dem Projekt-Protokoll (CLAUDE.md) komponiert, editierbar + re-injizierbar.
Plus per-Session-**Umgebung**: `workdir`, `coord_dir`/`sync_path`, Worktree-Konvention (heute hardcodiert
= bekannter Schmerz → WP4 zu Ende gedacht). **Fachliche Rollen sind KEIN Primitiv** — nur die
Koordinations-Rolle `coordinator|worker` ist eines (sie steuert merge-lock-Arbitrierung). Optionale
Rollen-**Templates** (architect/tester/reviewer/doc) als überschreibbare Presets. Optional M3+:
Rolle → Lease-Scope-Policy (Reviewer=nur Read-Leases) — opt-in, macht Rolle ≈ Capability-Scope.

Erzwungene Übergänge (Beispiele): `release(area)` nur wenn `lease.holder == caller`; `claim(area)`
schlägt fehl bei Overlap mit bestehendem Lease (inkl. **Pfad-Präfix-Overlap** — genau der Fehler, der
heute durch String-Vergleich durchrutscht, z. B. `kernel/src/embedded` ⊃ `kernel/src/embedded/usbd`);
`merge-unlock` nur durch Lock-Holder; Stale-Reclaim wenn `now - heartbeat > TTL`.

## 6. Neue Fähigkeiten, die der Port freischaltet

- **Pfad-Präfix-Overlap-Erkennung** beim Claim (heute manuell vom Koordinator abgefangen).
- **Abhängigkeits-/Blockade-Graph** + **Deadlock-Erkennung** (A hält X wartet Y / B hält Y wartet X).
- **Echte Merge-Queue** mit dem Singleton-Lock: FIFO, Rebase-Reihenfolge, CI-Gating, serielle
  Embed-Abhängigkeiten (genau die b→c-usbd-ELF-Serialisierung von Hand → strukturell).
- **Health-Dashboard:** Heartbeat-Frische, bald-stale-Leases, dunkle Sessions — live statt grep.
- **Rollen-/Policy-Durchsetzung:** „nur hub merged", „Worker ändern keine fremden Prioritäten" als
  *erzwungene* Regel statt CLAUDE.md-Konvention.
- **Replay/Zeitreise** aus dem typisierten Ledger („was tat die Fleet um 23:47?").
- **AP-/Backlog-Verknüpfung & Drift-Erkennung.** PR↔AP-Tagging (`concord claim … --ap B15.3`), Completion-
  Ledger (welcher Merge schließt welches AP, mit Evidenz-Backlink), „gemerged-aber-ungetickt"-Drift
  surfacen, und — *nachdem hub/Worker „done" behauptet* — den deterministischen `[ ]`→`[x]`-Edit in
  `BACKLOG.md` als Merge-Schritt anwenden. **Grenze:** die *Behauptung* „AP erfüllt" bleibt Urteil
  (Worker schlägt im PR vor, hub bestätigt am Merge); concordd verbucht nur. Integrations-Punkt:
  `BACKLOG.md` liegt im Ziel-Repo → Konvention + Repo-Zugriff oder Ledger→Doc-Sync-Hook.

## 7. CLI-Oberfläche & Hooks (Skizze)

```
concord <id> register|heartbeat|status|claim|release|merge-lock|merge-unlock|log|sync   # Parität
concord status --graph            # Lease-/Blockade-Graph (M3)
concord queue                     # Merge-Queue-Sicht (M3)
concord hook session-start|user-prompt|pre-tool|post-tool|statusline   # Hooks als Subcommands
concord daemon start|stop|status  # M2
concord mcp                       # MCP-Server-Modus (M3)
```
Claude Code ruft weiterhin nur ein Kommando auf — ob Shell oder Binary ist transparent.

## 8. Token-Kosten — was der Port konkret spart (Operator-Anliegen)

Der heutige Overhead steckt **fast ganz im Polling**, nicht in der Arbitrierung:
- **Idle-Self-Ticks** (~12 min, 6 Sessions): jeder Tick, der nichts findet, kostet einen Turn-Kontext
  für nichts → **größter vermeidbarer Posten**. M2 (Push) eliminiert die meisten: schlafen bis Ereignis.
- **Wachsender Prosa-Kanal** (O(n)-Read pro Tick): M3 (Inbox/Board) ersetzt „lies den ganzen Kanal"
  durch „hol meine Delta-Direktiven".
- Heartbeats sind schon billig (Hook, kein Modell-Turn).

**Sofort wirksam ohne Port** (Mitigationen): Tick-Intervall an Phase koppeln (ruhig 20–30 min, heiß
kürzer); Kanal-Hygiene; Delta-Reads erzwingen; Koordinator absorbiert die Denkarbeit zentral.
**Empfehlung:** in den ADR einen Messpunkt aufnehmen (eine Phase mit vs. ohne verkürzte Ticks
vergleichen), um den M2-Nutzen zu quantifizieren — heute fehlt Token-Telemetrie pro Session.

## 9. Migration / Cutover

1. M1 koexistiert mit Shell (gleiches Format). Differential-Tests beweisen Parität.
2. Hooks zeigen per Flag auf Shell *oder* Binary → schrittweise umschalten, jederzeit zurück.
3. Bei Parität: Shell-Version in `bin/legacy/` einfrieren, Binary wird Default.
4. M2+ baut additiv über dem unveränderten FS-Zustand auf — kein Format-Bruch nötig.

## 10. Risiken & Gegenmaßnahmen

| Risiko | Gegenmaßnahme |
|--------|---------------|
| Rewrite ersetzt die einfachen 20 %, lässt die schweren 80 % (Harness-Wecksemantik) liegen | M2 (Push) **ist** der schwere Teil — explizit als eigene Stufe, nicht „nice-to-have" |
| Daemon-Lifecycle-Last (start/stop/crash/upgrade) | Daemon **optional**; FS-Zustand bleibt autoritativ + ohne Daemon funktionsfähig |
| Scope-Creep über M1 hinaus | Harte Stop-Punkte; jede Stufe einzeln auslieferbar/abbrechbar |
| Differential-Drift Shell↔Rust während Koexistenz | Automatisierter Paritäts-Harness als CI-Gate in M1 |

## 11. Research vor dem ADR (Vision-Leitplanke: recherche-gestützt)

Vor der finalen ADR-Entscheidung gezielt Prior-Art spiegeln (noch **nicht** durchgeführt — bewusst
token-sparsam zurückgestellt; `hub` kann das als fokussierten Schritt liefern):
- **Lease + TTL + Stale-Reclaim:** wie etcd-Leases / Consul-Sessions / ZooKeeper-ephemeral-nodes das
  modellieren (Renew, Fencing-Tokens gegen Split-Brain). Extrahieren: Fencing-Token-Idee für „stale
  reclaim, aber alte Session schreibt noch".
- **fs-watch in Rust:** `notify`-Crate (Reife, Plattform-Parität, Debounce) für M2.
- **Single-Binary-Distribution:** `cargo-dist` / Release-Matrix-Patterns.
- **MCP-Server in Rust:** aktueller Stand offizieller/community SDKs für M3 §7.
Erkenntnisse **mit Quellen** im ADR festhalten.

## 12. Aufwand & Sequenzierung (grob)

- **M1** (CLI-Parität + typisierter Kern): **M** — der Großteil ist mechanische Verb-Portierung +
  Paritäts-Harness. Höchster Wert/Risiko-Schnitt.
- **M2** (Daemon/Push): **M** — technisch der anspruchsvollste Teil, größter Token-Nutzen.
- **M3** (Board/MCP/Inbox): **L** — schaltet die reichen Roadmap-Items frei.
- **M4** (Cross-Platform/Dist): **S–M** — größtenteils CI + Helper-Abstraktion (von Rust geschenkt).
- **M5** (Multi-Projekt/Dogfood): **S–M**. **M6** (auf-ais): post-1.0, separat.

**Empfohlener Schnitt für eine erste Freigabe:** **M1 + M2.** Das behebt die zwei realen Schmerzen
(Koordinator-Races + Poll-Token-Overhead) bei überschaubarem Risiko und lässt M3+ als spätere,
unabhängige Entscheidung offen.

## 13. Offene Fragen an den Operator

1. **Freigabe-Schnitt:** nur M1 (Parität, risikoarm) — oder M1+M2 (inkl. Token-Hebel)?
2. **Timing:** jetzt starten oder bis zum Abschluss der laufenden ais-Merge-Welle warten? (Concord ist
   nicht ais-kritisch — zieht aber, wenn gestartet, Session-Kapazität.)
3. **Research-Pass jetzt** (§11) als Vorbereitung des ADR — oder erst bei Go?
4. **Owner:** eine dedizierte Session für WP12, oder von `hub` nebenläufig gesteuert?
