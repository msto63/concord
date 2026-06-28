# Concord — Multi-Agent-Koordination für KI-Entwicklungsteams

> **Zweck:** Diese Anleitung beschreibt **Concord** — ein leichtgewichtiges, dateibasiertes System,
> mit dem **mehrere KI-Sessions (z. B. Claude Code)** gleichzeitig am *selben* Repository arbeiten,
> ohne sich gegenseitig zu beschädigen. Sie erklärt das Modell, die Mechanik und enthält die
> **fertigen Prompts**, um Concord für ein **neues** oder **bestehendes** Projekt aufzusetzen.
>
> Concord wurde im Projekt *ais* (ein Rust-Betriebssystem) entwickelt und über viele Wochen
> mit 5–6 parallelen Sessions erprobt. Die Anleitung ist projekt-unabhängig formuliert.

---

## 1. Was Concord ist — in einem Absatz

An einem Projekt arbeiten **mehrere KI-Sessions gleichzeitig**, jede in einem eigenen
**git-Worktree**. Ohne Koordination beschädigen sie sich: gleichzeitige Edits derselben Datei,
kollidierende Merges nach `main`, gegenseitiges Überschreiben. **Concord** verhindert das mit drei
einfachen Bausteinen: einer **strukturierten Registry + Bereichs-Leases + einer Merge-Sperre**
(via `coord.sh`), einem **Prosa-Kanal** für Diskussion (`SESSION-SYNC.md`) und einer **klaren
Rollen-/Kommunikationsstruktur** (ein menschlicher Auftraggeber → ein Koordinator → die Worker).
Es braucht keinen Server, keine Datenbank, kein `jq` — nur das gemeinsame Dateisystem.

---

## 2. Das Problem, das es löst

| Ohne Koordination | Mit Concord |
|---|---|
| Zwei Sessions editieren `main.rs` gleichzeitig → Konflikt/Verlust | **Lease** auf die Datei: die zweite Session sieht „CONFLICT" und koordiniert |
| Zwei Sessions mergen gleichzeitig nach `main` → kaputter Baum | **Merge-Sperre** (Singleton): nur eine Session merged zur Zeit |
| Eine Session „geht dunkel" (tut nichts mehr, niemand merkt's) | **Heartbeat + Self-Tick + sichtbare Status-Posts** machen Inaktivität sofort sichtbar |
| Sessions geben sich gegenseitig widersprüchliche Aufgaben | **Eine Stimme**: Aufgaben fließen nur Mensch → Koordinator → Worker |
| Eine Session höhlt eine Architektur-Invariante aus | **Vision-Leitplanken**: tragende Invarianten sind tabu für Shortcuts |

---

## 3. Das Modell

### 3.1 Rollen

- **Director (Mensch).** Gibt Ziele, Prioritäten, Richtungs-Entscheidungen. Spricht **nur mit dem
  Koordinator** (nicht mit jedem Worker einzeln). Im ais-Projekt: „mike".
- **K — Koordinator / Steward.** Eine dedizierte Session **ohne eigenes Code-Terrain**. Verteilt
  Aufgaben, sequenziert sie am kritischen Pfad, arbitriert Ownership-Streit, hält die Merge-Sperre,
  merged fertige PRs (mit stehender Freigabe des Directors), wacht über die Vision-Leitplanken,
  treibt die Auslastung. Eskaliert an den Director **nur** bei Meilensteinen, echten
  Richtungs-Entscheidungen oder unlösbaren Problemen.
- **Worker (A, B, C, …).** Je eine Session mit klarem **Terrain** (z. B. ein Subsystem, ein Daemon,
  eine Architektur-Schicht). Bauen, testen, liefern PRs. Verhandeln Schnittstellen **direkt**
  miteinander, aber nehmen Aufgaben/Prioritäten **nur über K**.

### 3.2 Git-Worktrees

Jede Session arbeitet in einem **eigenen git-Worktree** desselben Repos — so blockieren sich die
Arbeitsverzeichnisse nicht, und jede Session hat ihren eigenen Branch-Zustand:

```
git worktree add ../project-A   -b session-a/work
git worktree add ../project-B   -b session-b/work
git worktree add ../project-K   main          # K braucht oft nur Lesezugriff + Doku
```

Beispiel-Mapping (ais): `project` (main, Worker B), `project-split` (A), `project-b8` (C),
`project-native` (D), `project-k` (K). Ein Register dieser Zuordnung liegt **außerhalb** des Repos
(siehe `WORKTREES.md` im Coord-Verzeichnis).

### 3.3 Zwei Kanäle

1. **Strukturierte Registry** (`coord.sh` + ein State-Verzeichnis): *erzwungener* Zustand — wer
   ist aktiv, was ist geleast, wer merged gerade. Maschinen-lesbar, knapp.
2. **Prosa-Kanal** (`SESSION-SYNC.md`, **eine** Datei mit **absolutem** Pfad, für **alle**
   Worktrees dieselbe): *Diskussion, Begründung, Zuweisung, Fragen, Entscheidungen*. Append-only.

> **Begriffstrennung:** *Concord* = der strukturierte, erzwungene Koordinations-Zustand.
> *SESSION-SYNC.md* = der menschenlesbare Diskussions-Kanal daneben.

---

## 4. Die Mechanik

### 4.1 Das CLI — `coord.sh`

```
coord.sh register <id> "<focus>"    # einmal, bei Session-Start
coord.sh heartbeat <id>             # periodisch — hält dich „lebendig" (nur solange du ein Lease hältst)
coord.sh status                     # wer ist aktiv + was ist geleast + Merge-Sperre
coord.sh claim <id> <bereich> ["warum"]   # VOR dem Editieren einer geteilten Region
coord.sh release <id> <bereich>     # wenn fertig mit der Region
coord.sh merge-lock <id> ["warum"]  # VOR einem Merge nach main (Singleton)
coord.sh merge-unlock <id>          # nach dem Merge
coord.sh log <id> <event…>          # folgenreiche Absicht/Entscheidung strukturiert festhalten
```

Das vollständige Skript steht in **Abschnitt 12** — es ist projekt-unabhängig (Pfad per Env-Var
`AIS_COORD_DIR` überschreibbar).

### 4.2 Der State (ein Verzeichnis außerhalb des Repos)

```
<coord-dir>/
  sessions/<id>        # je Session: focus, started, heartbeat (Unix-Zeit)
  leases/<bereich>/    # je Lease: holder, why, since
  merge.lock/          # existiert ⇔ jemand merged gerade (holder, since)
  intents.jsonl        # append-only Log aller register/claim/merge/log-Events
  WORKTREES.md         # (manuell) welche Session in welchem Worktree
```

- **TTL = 30 min:** Ohne Heartbeat länger als 30 min gilt eine Session als **stale**; ihre Leases
  werden für andere **reclaimbar** (gewollte Crash-Recovery — eine abgestürzte Session blockiert
  nichts dauerhaft). `status` blendet stale Sessions/Leases automatisch aus.
- **Lease = kooperativer Anspruch**, kein harter Mutex. Erst die *CLAUDE.md-Pflicht* (Abschnitt 11.1)
  macht ihn verlässlich: jede Session liest sie beim Start und hält sich daran.

### 4.3 Der Prosa-Kanal `SESSION-SYNC.md`

- **Eine** Datei, **absoluter** Pfad, für **alle** Worktrees dieselbe (z. B.
  `/Users/you/Projects/<project>-SESSION-SYNC.md`).
- **Immer anhängen, nie überschreiben** (`cat >> …`, nicht ein Editier-Tool, das die ganze Datei neu
  schreibt — sonst gehen parallele Posts anderer Sessions verloren).
- **Ein Eintrag pro Beitrag**, Format:
  ```
  ### <absender> → <ziel>  (<knappes Thema>)
  <knapper, konkreter Inhalt>
  ```
  Beispiele für `<ziel>`: ein Session-Kürzel (`K`, `B`), `ALLE`, oder ein Status-Tag.
- **Status-/Entscheidungs-Tags** (Worker → K):
  ```
  ### B → K  (FERTIG | BLOCKED | IDLE: <was/warum>)
  ### B → K  (ENTSCHEIDUNG: <thema>)   ← mit Optionen + Empfehlung
  ### B → K  (WACH: <was ich aufnehme>)  ← ACK einer Zuweisung
  ```

---

## 5. Das Selbst-Antriebs-Problem (der wichtigste Teil)

**Eine KI-Session handelt nur, wenn ihr Harness ihr einen „Turn" gibt.** Nach einem Turn fällt sie in
**Dormanz**, bis ein Wecker sie re-invoziert. **Ein roher Hintergrund-Bash-Loop (`& while true …`)
weckt sie NICHT** (sein stdout versickert). Das ist die Hauptursache, warum Sessions „dunkel" gehen.

**Lösung — zwei Mechanismen, beide nötig:**

1. **Persistenter Watcher über das Harness-`Monitor`-Tool** (NICHT ein nacktes Bash-`while true`) auf
   **die eine** geteilte Kommunikationsdatei. Das Monitor-Tool macht aus jeder stdout-Zeile ein
   Weck-Event. Beispiel-Kommando (macOS — pollt die Datei-Änderungszeit):
   ```
   last=""; while true; do cur=$(stat -f %m /ABSOLUTER/PFAD/SESSION-SYNC.md 2>/dev/null); \
     [ "$cur" != "$last" ] && { [ -n "$last" ] && echo "SESSION-SYNC geändert"; last=$cur; }; sleep 30; done
   ```
   (Linux: `stat -c %Y` statt `stat -f %m`.)

2. **Eigener Self-Tick als Zuverlässigkeits-Netz** (Cron oder ein `/loop`, alle ~10–15 min): ein
   Timer kann nicht „verpasst" werden, ein Event schon. Pro Tick: heartbeaten (bei Lease), neue an
   dich gerichtete Direktiven aus SESSION-SYNC ziehen, zugewiesene Arbeit fortsetzen ODER Status posten.

> **Faustregel:** Event-Watcher = schnelle Reaktion; Self-Tick = Garantie, dass nichts liegenbleibt.
> Ohne beide driftet das Team auseinander.

---

## 6. Die Pflicht-Protokolle (jede Session)

1. **Beim Start registrieren** und **selbst-getrieben machen** (Abschnitt 5):
   ```
   coord.sh register <id> "<dein Fokus>"
   coord.sh status
   ```
2. **VOR dem Bearbeiten einer geteilten Region** ein Lease nehmen — und nur weitermachen, wenn es klappt:
   ```
   coord.sh claim <id> <pfad-oder-bereich> "warum"
   ```
   Bei `CONFLICT`: **nicht** trotzdem editieren — erst koordinieren (status + SESSION-SYNC, ggf.
   Director fragen). Nach Abschluss: `coord.sh release <id> <bereich>`.
3. **VOR einem Merge nach `main`** die Singleton-Merge-Sperre nehmen, danach freigeben:
   ```
   coord.sh merge-lock <id> "merge #NNN" && <merge-kommando> && coord.sh merge-unlock <id>
   ```
4. **Heartbeat — nur solange du ein Lease hältst** (im Turn-Rhythmus). Hältst du kein Lease, brauchst
   du keinen Heartbeat. **Kein abgekoppelter Dauer-Loop** (`nohup … sleep …`): der überlebt einen
   Crash und hält tote Leases ewig frisch → Stale-Reclaim wäre tot.
5. **Folgenreiche Entscheidungen** strukturiert loggen (`coord.sh log <id> "…"`) **und** die
   Begründung in den Prosa-Kanal (`### <id> → …`).
6. **Zuweisung quittieren (ACK):** auf einen Auftrag mit `### <id> → K (WACH: …)` antworten. Kein ACK
   binnen eines Ticks → K liefert neu / eskaliert.

---

## 7. Kommunikations-Regeln (Director → K → Worker)

- **Eine Stimme.** Aufgaben, Prioritäten, Reihenfolge fließen **Director → K → Worker**. **Keine
  Peer-to-Peer-Aufgabenvergabe** — keine Session gibt einer anderen Arbeit *vor* oder ändert deren
  Prioritäten. Brauchst du etwas von einer anderen Session → **läuft über K**.
- **Erlaubte Peer-Zusammenarbeit:** Schnittstellen/Verträge aushandeln (eine gemeinsame API, ein
  Wire-Format, ein Key-Schema), Infos teilen, Fragen stellen, Befunde übergeben — solange es die
  Prioritäten der anderen Session nicht umsteuert.
- **Ownership-Streit → K** entscheidet/moderiert (nicht untereinander ausfechten).
- **KEINE blockierenden Entscheidungs-Formulare in Worker-Sessions.** In einer Worker-Session schaut
  *niemand* live zu (der Director beobachtet nur K). Ein interaktiver Dialog (à la „bitte wählen")
  blockiert still und unsichtbar = Inaktivität. Stattdessen: die Entscheidung **mit Optionen +
  Empfehlung** als `### <id> → K (ENTSCHEIDUNG: …)` posten und an **anderer unblockierter Arbeit**
  weitermachen. Nur K (der mit dem Director spricht) darf interaktive Formulare nutzen.
- **Kein stilles Idlen.** Slice fertig / blockiert / keine Arbeit? → **sichtbar** an K melden
  (`### <id> → K (FERTIG | BLOCKED | IDLE: …)`). Ein aktiver Worker hält **entweder** ein Lease und
  arbeitet, **oder** hat einen offenen Status-Post bei K. Unsichtbarer Leerlauf ist ein Protokollbruch.
- **Kanal-Hygiene:** ein `### <id> → <ziel>`-Eintrag pro Beitrag, knapp und konkret; den Prosa-Kanal
  nicht zuspammen.

---

## 8. Vision-Leitplanken (Steward-Aufgabe von K)

Damit das Team unter Zeitdruck nicht die Architektur aushöhlt:

- **Tragende Invarianten sind tabu für Shortcuts.** Definiere zu Projektbeginn die nicht-verhandelbaren
  Garantien (im ais-Fall: Capability-Sicherheit, Datenklassifikation, Provenienz, Rechenschaft). K
  blockt jede Maßnahme, die sie aushöhlt — im Zweifel den harten, vision-treuen Weg.
- **Zwischenlösungen/Quick-Hacks sind erlaubt** (um Deadlocks zu vermeiden), **aber nur sichtbar und
  mit Aufräum-Pfad:** im Code markieren (`// HACK(<id> <datum>): <warum> — AUFRÄUMEN: <bedingung>`) UND
  auffindbar vermerken (Backlog-Eintrag oder `coord.sh log`).
- **Strategische Weichen recherche-gestützt:** bei Architektur-/Design-Entscheidungen vorher den Stand
  der Technik prüfen und die Erkenntnisse mit Quellen festhalten (z. B. in einem ADR).

---

## 9. Setup für ein NEUES Projekt — Schritt für Schritt

1. **Coord-Verzeichnis anlegen** (außerhalb des Repos, damit es nicht eingecheckt wird):
   ```
   mkdir -p ~/Projects/<project>-coord/sessions ~/Projects/<project>-coord/leases
   ```
2. **`coord.sh`** ins Repo legen (`tools/coord.sh`, Skript aus Abschnitt 12) und den Default-Pfad
   bzw. die Env-Var `AIS_COORD_DIR` auf dein Coord-Verzeichnis setzen. `chmod +x tools/coord.sh`.
3. **Prosa-Kanal anlegen** mit **absolutem** Pfad, außerhalb des Repos:
   ```
   touch ~/Projects/<project>-SESSION-SYNC.md
   ```
4. **Worktrees** für die geplanten Sessions anlegen (Abschnitt 3.2) + ein `WORKTREES.md` im
   Coord-Verzeichnis pflegen (welche Session wo).
5. **`CLAUDE.md`** ins Repo legen mit dem **Concord-Pflicht-Block** (Abschnitt 11.1) — diese Datei
   wird in *jede* Session automatisch geladen und ist der verbindliche Einbau ins Protokoll.
6. **K starten** (Koordinator) mit dem K-Wake-Prompt (11.2), dann die **Worker** mit dem
   Worker-Wake-Briefing (11.3). Jeder Session ihren **Self-Tick** einrichten (11.4 / Abschnitt 5).
7. **Director-Disziplin:** Du sprichst ab jetzt **mit K**, nicht mit jedem Worker.

## 10. Setup für ein BESTEHENDES Projekt

Identisch zu Abschnitt 9, plus:
- **Definiere die tragenden Invarianten** des Projekts explizit (Abschnitt 8) und schreib sie in die
  `CLAUDE.md` — sonst kann K die Leitplanken nicht durchsetzen.
- **Schneide die Terrains** entlang der vorhandenen Subsystem-Grenzen (ein Worker pro Modul/Schicht),
  damit Leases selten kollidieren.
- **Liste die „geteilten Regionen"** auf, die ein Lease verdienen (zentrale Dateien, die mehrere
  anfassen: Haupt-Einstiegspunkt, gemeinsame Build-Skripte, Doku, eingebettete Artefakte).

---

## 11. Die Prompts (Kern-Deliverable)

> Ersetze Platzhalter: `<project>` Projektname · `<coord-pfad>` absolutes Coord-Verzeichnis ·
> `<sync-pfad>` absoluter SESSION-SYNC.md-Pfad · `<invarianten>` die tragenden Garantien ·
> `<kritikpfad>` die wichtigste Abhängigkeitskette der offenen Arbeit.

### 11.1 Der `CLAUDE.md`-Concord-Block (wird in JEDE Session geladen)

```markdown
## ⚠️ Concord — Mehr-Session-Koordination (Pflicht)

An diesem Projekt arbeiten mehrere KI-Sessions gleichzeitig, jede in eigenem git-Worktree. Ohne
Koordination beschädigen sie sich. **Concord** verhindert das: strukturierte Registry + Bereichs-
Leases + Merge-Sperre. CLI: `tools/coord.sh` · Zustand: `<coord-pfad>` · Prosa-Kanal: `<sync-pfad>`.
Das ist nicht optional.

1. **Beim Start registrieren** (wähle ein stabiles Kürzel; frag den Director, falls unklar):
   `tools/coord.sh register <id> "<fokus>"` ; `tools/coord.sh status`.
   **Und mach dich SELBST-GETRIEBEN** (sonst gehst du dormant): (a) ein persistenter `Monitor`-Watcher
   (NICHT ein nacktes Bash-`while true`) auf `<sync-pfad>` (absolut, für alle Worktrees gleich);
   (b) ein eigener Self-Tick (Cron/`/loop`, ~10–15 min). Bei Event/Tick neue `### … → <id>`-Einträge
   lesen + reagieren. Zuweisung quittieren: `### <id> → K (WACH: …)`.
2. **VOR dem Bearbeiten einer GETEILTEN Region** ein Lease: `tools/coord.sh claim <id> <bereich> "warum"`.
   Bei `CONFLICT`: nicht editieren — erst koordinieren. Nach Abschluss: `release`.
3. **VOR einem Merge nach `main`** die Singleton-Merge-Sperre:
   `tools/coord.sh merge-lock <id> "…" && <merge> && tools/coord.sh merge-unlock <id>`.
4. **Heartbeat nur solange du ein Lease hältst** (im Turn-Rhythmus): `tools/coord.sh heartbeat <id>`.
   Kein abgekoppelter Dauer-Loop. `release`, sobald fertig.
5. **Folgenreiche Entscheidungen** loggen: `tools/coord.sh log <id> "…"` + Begründung als
   `### <id> → …` in `<sync-pfad>`.

## Kommunikation & Zuständigkeit (Director → K → Sessions)
- Aufgaben/Prioritäten fließen **Director → K → Worker**. KEINE Peer-Aufgabenvergabe. Brauchst du
  etwas von einer anderen Session → über K. Erlaubt: Schnittstellen aushandeln, Infos teilen.
- Ownership-Streit → K entscheidet. Merges → über K (Merge-Sperre).
- **KEINE blockierenden Entscheidungs-Formulare in Worker-Sessions** (niemand schaut zu = stille
  Inaktivität). Stattdessen: `### <id> → K (ENTSCHEIDUNG: <thema>)` mit Optionen + Empfehlung posten,
  an anderer Arbeit weiter. Nur K nutzt interaktive Formulare.
- **Kein stilles Idlen:** fertig/blockiert/keine Arbeit → sichtbar `### <id> → K (FERTIG|BLOCKED|IDLE: …)`.

## Vision-Leitplanken
Tragende Invarianten — **<invarianten>** — sind tabu für Shortcuts. Quick-Hacks nur sichtbar markiert
(`// HACK(<id> <datum>): … — AUFRÄUMEN: …`) + im Backlog vermerkt. Strategische Weichen recherche-gestützt.
```

### 11.2 K — Koordinator-Wake-Prompt (einmal beim Start von K)

```
Du bist Session K, der neutrale Concord-Koordinator/Steward für <project> (kein eigenes Code-Terrain).
Lies CLAUDE.md (Concord-Block). Registriere dich: tools/coord.sh register K "Koordinator/Steward".
Mach dich selbst-getrieben (Monitor-Watcher auf <sync-pfad> + Self-Tick alle ~10–15 min).

Deine Aufgaben:
- Aufgaben an Worker zuweisen + am kritischen Pfad sequenzieren (<kritikpfad>).
- Merge-ready, konfliktfreie, vision-treue PRs über die Merge-Sperre mergen (kein „noch nicht fertig"
  überfahren). Der Director hat dir Commits/PRs/Merges stehend delegiert.
- SESSION-SYNC lesen, auf Fragen/ENTSCHEIDUNG-Posts reagieren, Dissens moderieren, Ownership arbitrieren.
- Anti-Inaktivität: prüfe je zugewiesene Session, ob sie vorankommt (Lease + frischer Post). Aktiv-aber-
  idle oder neu stale → gezielter `### K → <id>`-Nudge. Über 2 Ticks dunkel trotz Nudge → an Director
  eskalieren (Kapazitätsproblem).
- Vision-Leitplanken durchsetzen: <invarianten> tabu für Shortcuts; Quick-Hacks nur markiert.
- MELDUNG AN DEN DIRECTOR NUR bei Meilenstein / echter Entscheidung für ihn / unlösbarem Problem —
  ausführlich + einfache Sprache. Routine (ACKs, Merges, Peer-Koordination) still handhaben. Alles ruhig
  + alle produktiv → knapp „Tick: ruhig".
```

### 11.3 Worker — Wake-Briefing (einmal beim Start jedes Workers)

```
Du bist Session <id>, ein Concord-Worker für <project>. Dein Terrain: <subsystem/aufgabe>.
Lies CLAUDE.md (Concord-Block). Registriere dich: tools/coord.sh register <id> "<fokus>".
Mach dich selbst-getrieben (Monitor-Watcher auf <sync-pfad> + Self-Tick alle ~10–15 min).

Arbeitsweise:
- Nimm Aufgaben NUR über K (aus SESSION-SYNC, an dich gerichtete `### … → <id>`-Einträge). Quittiere
  mit `### <id> → K (WACH: …)`.
- VOR dem Editieren einer geteilten Region: tools/coord.sh claim <id> <bereich> "warum". Bei CONFLICT
  nicht editieren — koordinieren. Heartbeat im Turn-Rhythmus solange du ein Lease hältst. Danach release.
- Schnittstellen mit anderen Workern DIREKT aushandeln (Peer), aber keine Prioritäten umsteuern.
- Liefere inkrementell als PR(s); bei echten Design-Weichen erst „Design-vor-Bau" als
  `### <id> → K (DESIGN: …)` posten. Verifiziere (Tests/Boot) VOR dem PR.
- KEINE interaktiven Entscheidungs-Formulare. Entscheidung nötig → `### <id> → K (ENTSCHEIDUNG: <thema>)`
  mit Optionen + Empfehlung, an anderer Arbeit weiter.
- Kein stilles Idlen: fertig/blockiert → `### <id> → K (FERTIG|BLOCKED|IDLE: …)`.
- Vision-Leitplanken: <invarianten> tabu für Shortcuts; Quick-Hacks nur markiert + vermerkt.

Erste Aufgabe: <konkreter erster Auftrag>.
```

### 11.4 Self-Tick-Prompt (Cron/`/loop`, je Session, ~10–15 min)

Für **K**:
```
Koordinator-Tick (Session K): 1) tools/coord.sh heartbeat K (falls Lease). 2) tools/coord.sh status —
wer aktiv/stale. 3) offene PRs: jeden merge-ready/konfliktfreien/vision-treuen über die Merge-Sperre
mergen. 4) neue SESSION-SYNC-Einträge seit letztem Tick lesen — auf Fragen/ENTSCHEIDUNG/Antworten
reagieren, Dissens moderieren. 5) Anti-Inaktivität: je zugewiesene Session prüfen (Lease + frischer
Post); idle/stale trotz Zuweisung → `### K → <id>`-Nudge; >2 Ticks dunkel → an Director eskalieren.
6) Stall am kritischen Pfad (<kritikpfad>)? → re-sequenzieren. 7) Meldung an Director nur bei
Meilenstein/Entscheidung/Problem. Alles ruhig → knapp „Tick: ruhig".
```

Für einen **Worker**:
```
Worker-Tick (Session <id>): 1) heartbeat falls Lease. 2) neue an dich gerichtete `### … → <id>`-
Einträge in SESSION-SYNC lesen + reagieren/ACKen. 3) zugewiesene Arbeit fortsetzen ODER, wenn fertig/
blockiert/leer, sichtbaren Status an K posten (`### <id> → K (FERTIG|BLOCKED|IDLE: …)`). 4) Lease/
Merge-Disziplin einhalten. Kein stilles Idlen.
```

---

## 12. Das `coord.sh`-Skript (vollständig, projekt-unabhängig)

> Den Default-Pfad in Zeile `COORD=…` auf dein Coord-Verzeichnis setzen (oder per Env-Var
> `AIS_COORD_DIR` überschreiben). Keine externen Abhängigkeiten (kein `jq`, kein Server).

```bash
#!/usr/bin/env bash
# Concord — dateibasierte Mehr-Session-Koordination. Verhindert, dass parallele
# KI-Sessions am selben Repo sich gegenseitig beschädigen/blockieren.
set -euo pipefail

COORD="${AIS_COORD_DIR:-/PFAD/ZU/<project>-coord}"
SESSIONS="$COORD/sessions"; LEASES="$COORD/leases"; LOG="$COORD/intents.jsonl"
TTL="${AIS_COORD_TTL:-1800}"   # 30 min ohne Heartbeat ⇒ Session/Lease stale
mkdir -p "$SESSIONS" "$LEASES"

now() { date +%s; }
slug() { printf '%s' "$1" | tr '/ ' '__'; }

session_stale() {                      # stale, wenn kein Heartbeat < TTL
    local f="$SESSIONS/$1"; [ -f "$f" ] || return 0
    local hb; hb=$(sed -n 's/^heartbeat=//p' "$f" 2>/dev/null || echo 0)
    [ -z "$hb" ] && return 0; [ $(( $(now) - hb )) -gt "$TTL" ]
}
logline() { local id="$1"; shift; printf '{"t":%s,"session":"%s","event":"%s"}\n' "$(now)" "$id" "$* " >> "$LOG"; }

cmd="${1:-status}"; shift || true
case "$cmd" in
  register)
    id="${1:?session id}"; focus="${2:-}"
    printf 'focus=%s\nstarted=%s\nheartbeat=%s\n' "$focus" "$(now)" "$(now)" > "$SESSIONS/$id"
    logline "$id" "register: $focus"; echo "registered '$id' (focus: $focus)"; "$0" status ;;
  heartbeat)
    id="${1:?session id}"
    if [ -f "$SESSIONS/$id" ]; then
        f=$(sed -n 's/^focus=//p' "$SESSIONS/$id"); s=$(sed -n 's/^started=//p' "$SESSIONS/$id")
        printf 'focus=%s\nstarted=%s\nheartbeat=%s\n' "$f" "$s" "$(now)" > "$SESSIONS/$id"
    else printf 'focus=\nstarted=%s\nheartbeat=%s\n' "$(now)" "$(now)" > "$SESSIONS/$id"; fi ;;
  status)
    echo "── Concord ($COORD) ──"; echo "ACTIVE SESSIONS:"
    for f in "$SESSIONS"/*; do [ -e "$f" ] || { echo "  (none)"; break; }
        id=$(basename "$f"); session_stale "$id" && continue
        printf '  %-10s focus: %s\n' "$id" "$(sed -n 's/^focus=//p' "$f")"; done
    echo "HELD LEASES:"; any=0
    for d in "$LEASES"/*; do [ -e "$d" ] || break
        area=$(basename "$d"); holder=$(cat "$d/holder" 2>/dev/null || echo '?')
        session_stale "$holder" && continue
        printf '  %-28s by %s — %s\n' "$area" "$holder" "$(cat "$d/why" 2>/dev/null)"; any=1; done
    [ "$any" = 0 ] && echo "  (none)"
    if [ -d "$COORD/merge.lock" ]; then mh=$(cat "$COORD/merge.lock/holder" 2>/dev/null || echo '?')
        session_stale "$mh" || echo "MERGE LOCK: held by $mh"; fi ;;
  claim)
    id="${1:?session id}"; area=$(slug "${2:?area}"); why="${3:-}"; d="$LEASES/$area"
    if mkdir "$d" 2>/dev/null; then
        echo "$id">"$d/holder"; echo "$why">"$d/why"; echo "$(now)">"$d/since"
        logline "$id" "claim: $2 ($why)"; echo "CLAIMED $2"
    else holder=$(cat "$d/holder" 2>/dev/null || echo '?')
        if [ "$holder" = "$id" ]; then echo "already yours: $2"; exit 0; fi
        if session_stale "$holder"; then
            echo "$id">"$d/holder"; echo "$why">"$d/why"; echo "$(now)">"$d/since"
            logline "$id" "reclaim-stale: $2"; echo "RECLAIMED $2 (stale $holder)"
        else echo "CONFLICT: '$2' leased by '$holder' — coordinate first"; exit 2; fi; fi ;;
  release)
    id="${1:?session id}"; area=$(slug "${2:?area}"); d="$LEASES/$area"
    [ -d "$d" ] && rm -rf "$d" && logline "$id" "release: $2" && echo "released $2" || echo "no lease on $2" ;;
  merge-lock)
    id="${1:?session id}"; why="${2:-}"; d="$COORD/merge.lock"
    if mkdir "$d" 2>/dev/null; then echo "$id">"$d/holder"; echo "$(now)">"$d/since"
        logline "$id" "merge-lock: $why"; echo "MERGE LOCK acquired"
    else holder=$(cat "$d/holder" 2>/dev/null || echo '?')
        if [ "$holder" = "$id" ] || session_stale "$holder"; then
            echo "$id">"$d/holder"; echo "$(now)">"$d/since"; echo "MERGE LOCK (re)acquired"
        else echo "MERGE LOCK held by '$holder' — wait"; exit 2; fi; fi ;;
  merge-unlock)
    id="${1:?session id}"; rm -rf "$COORD/merge.lock"; logline "$id" "merge-unlock"; echo "merge lock released" ;;
  log)
    id="${1:?session id}"; shift; logline "$id" "$*"; echo "logged" ;;
  *) echo "unknown command: $cmd"; exit 1 ;;
esac
```

---

## 13. Lessons Learned / Gotchas (aus dem Live-Betrieb)

- **#1-Fehler: worktree-relativer Pfad zum Prosa-Kanal.** Der SESSION-SYNC-Pfad MUSS für alle Sessions
  **absolut + identisch** sein. Ein relativer Pfad → jede Session schreibt in ihre eigene Kopie → das
  Team driftet auseinander.
- **Rohe `& while true`-Loops wecken nichts.** Nur das Harness-`Monitor`-Tool macht aus stdout ein
  Weck-Event. Plus immer einen Self-Tick als Netz.
- **SESSION-SYNC immer anhängen** (`cat >> …`), nie mit einem Editier-Tool die ganze Datei neu
  schreiben — sonst überschreibst du parallele Posts.
- **Backticks in `coord.sh log "…"` vermeiden** — in doppelten Anführungszeichen lösen sie
  Command-Substitution aus (Parse-Fehler). Klartext loggen.
- **Heartbeat nur bei Lease, und gekoppelt an echte Aktivität** (im Turn). Ein abgekoppelter
  Dauer-Heartbeat hält tote Leases ewig frisch → die Crash-Recovery (Stale-Reclaim) stirbt.
- **K hält kein Code-Terrain.** Wenn der Koordinator selbst entwickelt, verliert er die Neutralität
  für Merge-Arbitrierung + Vision-Stewardship. Doku/Backlog darf K pflegen (über die Merge-Sperre).
- **Cross-Worktree-Edit-Falle:** editiere Repo-Dateien immer in *deinem* Worktree, nie im Worktree
  einer anderen Session (sonst kollidierst du mit deren uncommittetem Stand).
- **„Stale-Base"-Anzeige beim Merge:** wird ein Branch von einem hinterherhinkenden lokalen `main`
  erstellt, zeigt die Merge-Ausgabe evtl. mehr geänderte Dateien an, als der PR real enthält — die
  autoritative Wahrheit ist der PR-Diff gegen das echte `main` (immer gegen-verifizieren).
- **Identität NIE aus dem Verzeichnis ableiten.** Mehrere Sessions können aus *demselben* cwd laufen
  (hier: alle aus dem main-Repo). `git --show-toplevel`/cwd identifiziert die Session dann NICHT — eine id-Quelle
  muss explizit sein (`CONCORD_ID`-Env beim Start). Erster Anlauf gab jede Session als dieselbe id aus.

---

## 14. Automatisierungs-Schicht — Hooks + Statusline (umgesetzt 2026-06-28)

Die Schwäche des Kerns (Abschnitte 1–13): er lebt davon, dass jede Session **daran denkt**, `coord.sh`
aufzurufen. **Claude Codes Hooks + Statusline verschieben das Protokoll von „erinnert" zu „erzwungen +
sichtbar".** Diese Schicht ist optional, aber sie nimmt die meiste Disziplin-Last weg.

**Prinzip:** Die Skripte liegen außerhalb des Repos (`<coord-dir>/hooks/`), sind **fail-open**
(jeder Fehler lässt die Session normal weiterarbeiten) und **self-gating**.

> **⚠️ Identitäts-Quelle = die Env-Variable `CONCORD_ID` (beim Start gesetzt), NICHT das Verzeichnis.**
> Jede Session wird so gestartet:
> ```
> CONCORD_ID=E claude        # für Session E   ·   CONCORD_ID=K claude  für K   usw.
> ```
> Die Hooks lesen `$CONCORD_ID` (vererbt an die Hook-Subprozesse). Ist sie nicht gesetzt → die Hooks
> sind **No-op** (nie eine *falsche* id). **Warum nicht das Verzeichnis?** Eine harte Lektion aus dem
> Live-Betrieb: hier laufen **alle** Sessions aus *demselben* Verzeichnis (dem main-Repo) — das cwd kann
> die logische Session also *nicht* unterscheiden. Eine Ableitung über `git --show-toplevel`/`worktree-map`
> ergab fälschlich für *jede* Session dieselbe id. Die Env-Variable ist die robuste, eindeutige Quelle.
> *(Bestätigt 2026-06-28: `CONCORD_ID=E` → die Statusline zeigt korrekt `● E`.)*

### 14.1 Die Skripte (`<coord-dir>/hooks/`)

| Datei | Hook-Event | Was es tut |
|---|---|---|
| `shared-regions` | — | **enge** Liste der Datei-Pfade, die ein Lease-Enforcement verdienen (nur echt-heiße Dateien) |
| `lib.sh` | — | gemeinsame Helfer: **`concord_id` = `$CONCORD_ID`** (Env, siehe oben), Farbe je Session, Slug, Registry-Feld lesen |
| `worktree-map` | — | *(veraltet — Identität läuft jetzt über `CONCORD_ID`; nur noch optionale Doku/Fallback)* |
| `statusline.sh` | `statusLine` | rendert pro Fenster: **`● <id> · <fokus> · ⚷<lease> · ♥<hb-alter>`**, farbcodiert je Session (löst „welche Session sehe ich") |
| `session-start.sh` | `SessionStart` | **Auto-Register** (falls neu) bzw. Heartbeat; sagt dem Modell seine id + verweist auf SESSION-SYNC |
| `post-tool.sh` | `PostToolUse` (alle Tools) | **Heartbeat bei jeder Tool-Nutzung** → „lebendig" koppelt automatisch an echte Aktivität |
| `user-prompt.sh` | `UserPromptSubmit` | injiziert je Turn kompakt: **aktive Sessions + mein Lease + Merge-Sperre + NEUE `### … → <id>/ALLE`-Direktiven** seit letztem Turn (keine verpassten Broadcasts) |
| `pre-tool.sh` | `PreToolUse` (Edit/Write/MultiEdit/Bash) | **Lease-Enforcement**: blockt (exit 2) einen Edit an einer geteilten Region nur, wenn eine *andere, aktive* Session das Lease hält; **Merge-Singleton**: blockt ein `gh pr merge`/`git merge`, wenn eine andere aktive Session die Merge-Sperre hält. **Default-allow** bei jeder Unsicherheit. |

### 14.2 Konfiguration (`~/.claude/settings.json`)

Global einmal eingetragen — wirkt nur in Sessions, die mit `CONCORD_ID=<id>` gestartet wurden (sonst No-op,
keine Wirkung auf Fremdprojekte):

```json
{
  "statusLine": { "type": "command", "command": "<coord-dir>/hooks/statusline.sh" },
  "hooks": {
    "SessionStart":     [ { "hooks": [ { "type": "command", "command": "<coord-dir>/hooks/session-start.sh" } ] } ],
    "UserPromptSubmit": [ { "hooks": [ { "type": "command", "command": "<coord-dir>/hooks/user-prompt.sh" } ] } ],
    "PostToolUse":      [ { "hooks": [ { "type": "command", "command": "<coord-dir>/hooks/post-tool.sh" } ] } ],
    "PreToolUse":       [ { "matcher": "Edit|Write|MultiEdit|NotebookEdit|Bash",
                            "hooks": [ { "type": "command", "command": "<coord-dir>/hooks/pre-tool.sh" } ] } ]
  }
}
```

> **⚠️ Self-Modification-Gate:** Das Eintragen dieser Hooks ändert die *eigene* Startkonfig + das
> Tool-Gating des Agenten — Claude Codes Sicherheits-Klassifizierer blockt das, solange der Nutzer es
> nicht **ausdrücklich** freigibt. Darum: **vorher `~/.claude/settings.json` sichern** und den Merge
> **bewusst** ausführen (per `python3`-Merge, der bestehende Keys erhält), nicht beiläufig.

### 14.3 Sicherheits-Design (warum es nicht nach hinten losgeht)

- **fail-open überall:** kein Hook darf eine Session lahmlegen — jeder Fehler endet in `exit 0`.
- **self-gating via `CONCORD_ID`:** wirkt nur in Sessions, die mit der Env-Variable gestartet wurden;
  sonst sofort No-op (nie eine *falsche* id — die Lektion aus dem ersten Anlauf, wo die cwd-Ableitung jede
  Session als dieselbe id ausgab).
- **`pre-tool.sh` ist default-allow:** blockt nur bei *sicherem* Konflikt mit einer *aktiven* Fremd-
  Session (Heartbeat < 30 min); stale Leases blocken nie. Die `shared-regions`-Liste **eng** halten,
  sonst nerven Fehl-Blocks. Override jederzeit: Lease freigeben/neu zuweisen oder Region aus der Liste.
- **getestet vor dem Scharfschalten:** Syntax (`bash -n`), Funktion (Statusline/Status-Injektion),
  und die Block-Logik **isoliert** (temp-COORD mit Fremd-Lease → exit 2; eigenes Lease → exit 0).

### 14.4 Roadmap — was noch fehlt (Punkte 5–8)

| # | Ausbau | Nutzen | Aufwand |
|---|---|---|---|
| ✅ 1–4 | **Statusline + Auto-Register/Heartbeat + Status-Injektion + Lease/Merge-Enforcement** | Fenster-Identität, keine Disziplin-Last, keine verpassten Broadcasts, erzwungene Leases | umgesetzt |
| ✅ 5 | **`concord`-CLI: `start`/`stop`/`pause`/`resume`/`dash`** + Identität via `CONCORD_ID` + **Worktree-Konvention `<repo>-<id>`** + **READY/GO-Dispatch-Handshake** + Start **im aktuellen Terminal** | „Mission Control": ein Befehl je Session (eigener Worktree + volle Rechte + /loop); K dispatcht per GO | umgesetzt 2026-06-28 |
| ⬜ 6 | **Strukturiertes Board** (`<coord-dir>/board.jsonl` + `concord board`) | Gesamtübersicht aller **Arbeitspakete → Tasks** mit Status × Priorität × Agent; K setzt Prio, Owner kippt Status | ~1 Tag |
| ⬜ 7 | **Concord-MCP-Server** | register/claim/merge-lock/status/board als *typisierte* Tools statt Shell; speist Board/Dashboard | später, wenn 5–6 wachsen |
| ⬜ **8** | ⭐ **Multi-Projekt-Fähigkeit** (mike-Wunsch 2026-06-28) | *zwei+ Projekte gleichzeitig mit Concord, sauber isoliert.* Heute hartkodieren die **Hooks** den ais-Sync-Pfad (`user-prompt.sh`/`session-start.sh`) → ein 2. Projekt vermischt sich. **Nötig:** (a) Hooks ent-hartkodieren → `CONCORD_SYNC`/`CONCORD_DIR` aus Env lesen; (b) `concord` leitet **pro Projekt aus dem Projekt-Root ab** (`<repo>-coord` + `<repo>-SESSION-SYNC.md`) und **injiziert die Env beim Start**, sodass die globalen Hooks per geerbter Env die richtige Projekt-Koordination nutzen; (c) `AIS_*`→`CONCORD_*` umbenennen; (d) optional `concord init`. **Isolation = Coord-Dir + Sync-Datei pro Projekt** (die Session-id allein trennt NICHT — beide Projekte dürfen ein „E" haben). | ~0,5–1 Tag |

**Pause-Mechanik (für #5):** ein Flag `<coord-dir>/paused/<id>`; der Self-Tick + `user-prompt.sh`
prüfen es → die Session heartbeatet nur, arbeitet nicht, bis `concord resume <id>` das Flag entfernt.

**Board-Format (für #6):** je Task eine JSON-Zeile `{wp, id, titel, status, prio, owner, pr, deps}`;
`concord board` rendert nach Arbeitspaket gruppiert. Alternativ GitHub Issues+Projects (Labels=Status/
Prio, Assignee=Session) für ein fertiges Web-Board. *Hinweis:* Claude Codes `TaskCreate`/`TaskUpdate`
sind **pro Session** (nicht geteilt) — für ein geteiltes Board braucht es die Datei oder GitHub.

---

*Concord ist absichtlich minimal: Dateien + ein Shell-Skript + Disziplin per CLAUDE.md. Kein Server,
keine Datenbank — damit es auf jedem geteilten Dateisystem sofort läuft und nie selbst zum Single Point
of Failure wird.*
