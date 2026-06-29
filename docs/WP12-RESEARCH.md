# WP12 — Research Pass: Concord → Rust Port (ADR-Vorrecherche)

> Fokussierter, quellengestützter Research-Pass für die Architektur-Entscheidung,
> das Multi-Session-Koordinationstool **Concord** (heute Shell + Dateisystem-Zustand)
> nach Rust zu portieren. Stand: 2026-06-29. Jede Behauptung mit Quelle; offene/
> umstrittene Punkte sind markiert.
>
> Vision-Kontext: das Tool *erzwingt* Koordination (Leases ≈ Capabilities, Merge-Sperre,
> Rechenschafts-Ledger). Hauptziele des Ports: **Push statt Poll** und **typisierter Zustand**.

---

## 1. Lease/Lock + TTL + Stale-Reclaim ohne Split-Brain

**Wie die großen Systeme zeitbasierte Leases + Renewal + Auto-Reclaim modellieren:**

- **etcd — Lease + KeepAlive.** Client ruft `Grant(ttl)` → bekommt `lease_id`; jeder Key
  kann an die Lease gehängt werden. Ein bidirektionaler **KeepAlive-Stream** hält sie am
  Leben; bricht der Stream ab oder verstummt der Client, **läuft die Lease ab und alle
  angehängten Keys werden automatisch gelöscht** (= Crash-Recovery ohne Operator). Das ist
  exakt das Concord-Pattern „kein Heartbeat > 30 min → Lease reclaimbar".
- **ZooKeeper — ephemeral znodes.** Znodes, die an eine Session gebunden sind und
  **automatisch verschwinden, wenn die Session endet** (absichtlich oder durch Crash). Kein
  expliziter TTL, sondern session-gebunden über Client-KeepAlives.
- **Consul — Sessions + zeitlich begrenzte Leases.** Keys können an eine Session gebunden
  werden; läuft die Session ab (kein Renewal), werden die Locks freigegeben. Konzeptionell
  wie etcd, mit Health-Check-Integration.
- **Gemeinsamer Kern:** *TTL + Renewal-Kanal + automatische Freigabe bei Ausbleiben des
  Renewals.* Concords „heartbeat sonst stale-reclaim" ist genau dieses Muster, nur lokal.

**Fencing-Token-Pattern (Martin Kleppmann, „How to do distributed locking", 2016):**

- **Warum TTL allein unsicher ist:** Ein Lease-Halter kann pausieren (GC-Pause, CPU-Scheduling,
  Netz-Delay) und die Lease läuft *während* der Pause ab. Beispiel aus dem Artikel: „Client 1
  acquires the lease and gets a token of 33, but then it goes into a long pause and the lease
  expires. Client 2 acquires the lease, gets a token of 34." → **beide glauben kurzzeitig, den
  Lock zu halten** = Split-Brain / konkurrierende Writes.
- **Die Lösung:** Bei jeder Lock-Akquise vergibt der Lock-Service einen **monoton steigenden
  Fencing-Token**. Jeder Write an die geschützte Ressource trägt diesen Token mit. Die Ressource
  **merkt sich den höchsten gesehenen Token und lehnt jeden Write mit niedrigerem Token ab**:
  „the storage server remembers that it has already processed a write with a higher token number
  (34), and so it rejects the request with token 33."
- **Kernidee:** Der Token erzeugt eine **zeit-unabhängige totale Ordnung** — Sicherheit hängt
  nicht mehr von Timing-Annahmen ab. Eine totgeglaubte, aber noch laufende Session kann nach dem
  Reclaim **nicht** mehr schreiben, weil ihr alter Token vom Wächter rejected wird.
- **etcd liefert das gratis:** Jede Key-Änderung bekommt eine global monoton steigende
  **Revision**, die direkt als Fencing-Token taugt — laut Vergleichsquellen ein Hauptvorteil von
  etcd-Locks gegenüber z. B. Redlock (das *keine* Fencing-Tokens erzeugt, Kleppmanns zentrale
  Redlock-Kritik).

**Übertragbar auf lokales (single-host, Multi-Prozess) Concord:**

- Single-Host eliminiert Netz-Partitionen, **aber NICHT das Pausen-Problem** (eine Claude-Session
  fällt nach einem Turn in Dormanz, ein Bash-Prozess kann pausiert/`SIGSTOP`/swap-blockiert sein) —
  also bleibt Stale-Reclaim-nach-Pause real und damit Fencing relevant.
- **Konkret:** Concord vergibt pro Lease einen monotonen `epoch`/`fence`-Zähler (z. B.
  global hochzählend, persistiert im Ledger). Jede folgenreiche Aktion (Merge, geteilter Edit,
  Release) trägt ihren Fence mit; der zentrale Zustand (das `coord`-Daemon/-Ledger) **lehnt
  Aktionen mit veraltetem Fence ab**. Damit kann eine nach Stale-Reclaim wieder erwachte Session
  keinen Merge/Write mehr durchdrücken — sie wird hart abgewiesen statt „höflich gebeten".
- Da Concord ohnehin einen **zentralen Serialisierungspunkt** haben wird (Daemon mit typisiertem
  Zustand), ist die Fencing-Prüfung billig: ein `if action.fence < state.max_fence { reject }`.
  Das ist die single-host-Entsprechung der etcd-Revision.

**Quellen:**
- https://martin.kleppmann.com/2016/02/08/how-to-do-distributed-locking.html
- https://etcd.io/docs/v3.5/learning/why/
- https://www.youngju.dev/blog/architecture/2026-03-12-distributed-lock-redis-redlock-zookeeper-etcd-comparison.en
- https://singhajit.com/distributed-systems/lease/
- https://surfingcomplexity.blog/2025/03/03/locks-leases-fencing-tokens-fizzbee/

---

## 2. Rust fs-watch für einen Push-Notify-Daemon (`notify`)

- **Reife/Adoption:** `notify` ist der De-facto-Standard. Verwendet von alacritty, cargo-watch,
  deno, mdBook, **rust-analyzer**, watchexec, watchfiles, xi-editor u. a. Aktiv gepflegt
  (notify 8.x in 2025; notify-types v2.1.0 Jan 2026; **MSRV 1.88**, Policy: aktuelle stable + 2
  vorherige). Reif und breit erprobt.
- **Plattform-Backends:** Linux/Android = **inotify**; macOS = **FSEvents** (oder kqueue via
  Feature); Windows = **ReadDirectoryChangesW**; BSD/iOS = **kqueue**; überall **PollWatcher** als
  Fallback. Einheitliche API über alle.
- **Bekannte Fallstricke (wichtig für einen verlässlichen Daemon):**
  - Der **Linux/inotify-Backend ist laut Doku ausdrücklich keine 100 % zuverlässige Quelle** —
    Events können unter Last verloren gehen.
  - **macOS-FSEvents-Security-Modell:** Events auf Dateien, die *nicht dir gehören*, sind teils
    nicht beobachtbar; Workaround ist der **PollWatcher** (kleiner Performance-Preis). Für einen
    Zustands-Ordner, den derselbe User schreibt, in der Regel unkritisch.
  - **Event-Verstärkung/Batching:** Ein einzelnes Save erzeugt auf inotify oft **3–5 Events**,
    auf FSEvents **gebatchte Events mit mehrdeutigem Typ**. → **Debouncing ist Pflicht**, nicht
    Kür, sonst feuert der Daemon mehrfach pro logischer Änderung.
- **Debouncing:** zwei offizielle Begleiter — **`notify-debouncer-mini`** (leichtgewichtig,
  Zeitfenster-Zusammenfassung) und **`notify-debouncer-full`** (zustandsbehaftet, dedupliziert,
  tracked Rename/Move zuverlässiger). Für Concord (Ordner + eine Markdown-Datei, Push-Stream)
  empfiehlt sich **`notify-debouncer-full`**, weil es Rename/Move und Mehrfach-Events sauber
  konsolidiert.
- **Empfohlenes Muster:** einen `RecommendedWatcher` rekursiv auf den **Zustands-Ordner**
  (`ais-coord/`) plus expliziter Watch auf die **`SESSION-SYNC.md`** legen; Achtung: viele Editoren
  schreiben Dateien atomar via *rename-over* (temp → rename), wodurch der ursprüngliche inode/Watch
  verschwinden kann — daher den **enthaltenden Ordner** beobachten (robuster als nur die Datei) bzw.
  `debouncer-full` nutzen, das diese Rename-Semantik kennt. (Letzteres ist gängige Praxis, im
  GitHub-Excerpt aber nicht wörtlich belegt — als *Engineering-Hinweis* markiert, nicht als Zitat.)
- **Alternativen (kurz):** `watchexec`-Crate (höhere Abstraktion, baut auf notify auf — gut, wenn
  man eh Prozess-Restart-Semantik will); direktes `inotify`/`kqueue`/`fsevent`-binding (mehr
  Kontrolle, mehr Plattform-Code, kein Cross-Platform-Gewinn); reines Polling (nur wenn Push
  unzuverlässig ist). Für „ein Daemon streamt Events aus einem Ordner" ist **notify +
  debouncer-full** die naheliegende, risikoarme Wahl.

**Quellen:**
- https://github.com/notify-rs/notify
- https://docs.rs/notify
- https://crates.io/crates/notify
- https://oneuptime.com/blog/post/2026-01-25-file-watcher-debouncing-rust/view

---

## 3. Single-Binary-Distribution für ein Rust-CLI

- **`cargo-dist` (axodotdev) als Standard-Werkzeug.** „Shippable application packaging for Rust".
  Deckt zwei Phasen ab: **Build** (Release planen, Binaries + Installer bauen) und **Distribute**
  (Artefakte hosten, Pakete publishen, Release announcen).
- **CI-Generierung:** `cargo dist init` ist ein interaktiver Assistent, der **`release.yml`** für
  GitHub Actions generiert — die volle Pipeline *plan → build → host → publish → announce*. Man
  aktiviert GitHub-CI und typischerweise den **Shell-Installer** (curl-bare install script).
- **Voraussetzung:** `repository`-URL in `Cargo.toml` setzen (cargo-dist leitet daraus die
  Release-/Installer-URLs ab).
- **Cross-Compilation / Release-Matrix (Stand 2025+):** erweiterte Cross-Compile-Unterstützung für
  **Linux via `cargo-zigbuild`** und **Windows via `cargo-xwin`**; deckt die macOS/Linux/Windows-
  Matrix in einem GitHub-Release ab. Neuere Versionen erzeugen zudem **CycloneDX-SBOMs** (`bom.xml`)
  für Supply-Chain-Transparenz — passt gut zur „Rechenschafts"-Ausrichtung des Projekts.
- **Best Practice 2025/2026 (Konsens aus Orhun Parmaksız' Release-Guide + cargo-dist-Tips):**
  cargo-dist für die **GitHub-Release-Binärmatrix + Installer**, optional kombiniert mit
  **`cargo-release`** (crate-ci) für Versionsbump/Tag/Changelog-Workflow und `cargo install` als
  niedrigschwelligen Pfad für Rust-Nutzer. cargo-dist und cargo-release sind komplementär
  (Distribution vs. Versionierungs-Choreografie).
- **Für Concord konkret:** ein einzelnes statisch gelinktes CLI (`concord`/`coord`) pro
  Ziel-Triple, Shell-Installer für macOS (das ist hier die Hauptplattform — Apple Silicon),
  optional `cargo install` für Mitentwickler. SBOM mitnehmen.
- **Hinweis/Vorsicht:** cargo-dist (axodotdev) hat in der Vergangenheit Maintenance-/Governance-
  Turbulenzen gehabt; vor dem Commit kurz den aktuellen Repo-Status prüfen. Für ein *single-host,
  ein-Plattform*-Tool ist der volle cargo-dist-Apparat ggf. überdimensioniert — ein simples
  `cargo build --release` + GitHub-Actions-Matrix kann genügen. (Abwägung, nicht eindeutig — als
  offener Punkt markiert.)

**Quellen:**
- https://github.com/axodotdev/cargo-dist
- https://crates.io/crates/cargo-dist
- https://blog.orhun.dev/automated-rust-releases/
- https://sts10.github.io/docs/cargo-dist-tips.html
- https://rust-cli.github.io/book/tutorial/packaging.html
- https://github.com/crate-ci/cargo-release

---

## 4. MCP-Server in Rust (`rmcp` — offizielles SDK)

- **Offiziell + reif.** `rmcp` ist das **offizielle Rust-SDK** des Model Context Protocol
  (`github.com/modelcontextprotocol/rust-sdk`). **Stand stabil:** Tag **`rmcp-v1.8.0`
  (23. Juni 2026)**, 80 Releases — also bereits jenseits 1.0, aktiv gepflegt. Production-tauglich,
  async-first auf **tokio**.
- **Typisierte Tool-Endpunkte (genau Concords Bedarf):** Tools werden über die Makros
  **`#[tool]`, `#[tool_router]`, `#[tool_handler]`** definiert; Argumente sind **typisierte Structs**
  mit `Deserialize` + `JsonSchema`. Das Makro **generiert Input/Output-Schema, `list_tools()` und
  `call_tool()` automatisch** — kein handgeschriebenes JSON-Schema, keine Boilerplate. Damit lassen
  sich `register`/`claim`/`merge-lock`/`status` als **stark typisierte, schema-validierte Endpunkte**
  abbilden — exakt das Ziel „typisierter Zustand statt Stringly-typed Shell".
- **Transporte:** Kern-SDK dokumentiert **stdio** und **TokioChildProcess** (Subprozess); für ein
  lokales Tool, das Claude-Sessions als MCP-Server bedient, ist **stdio der naheliegende Transport**.
  *Streamable HTTP / SSE* existiert, aber eher über Erweiterungen (`rmcp-actix-web`) als im Kern —
  für Concord (lokal, ein Host) nicht nötig.
- **Caveats:** (1) **1.x brachte Breaking Changes** — Migrations-Guide beachten, API noch in
  Bewegung über Major-Versionen. (2) **Sampling, Roots, Logging sind deprecated** (per SEP-2577) —
  beim Design nicht darauf bauen. (3) **tokio-Pflicht** (async runtime).
- **Einschätzung für Concord:** Gut geeignet. Das Tool kann denselben typisierten Zustands-Kern
  einmal definieren und sowohl als **CLI** (Thema 3) als auch als **MCP-Server** (rmcp) exponieren —
  Sessions koordinieren dann über typisierte Tool-Calls statt `coord.sh`-Strings, was Push-Notify
  (Thema 2: Server pusht Lease-/SYNC-Änderungen als MCP-Notifications/Resource-Updates) natürlich
  ergänzt. Tutorials (Shuttle) bestätigen stdio-Server in Rust als geradlinig.

**Quellen:**
- https://github.com/modelcontextprotocol/rust-sdk
- https://docs.rs/rmcp/latest/rmcp/
- https://deepwiki.com/modelcontextprotocol/rust-sdk
- https://hackmd.io/@Hamze/S1tlKZP0kx
- https://www.shuttle.dev/blog/2025/07/18/how-to-build-a-stdio-mcp-server-in-rust

---

## Synthese (für das ADR)

Die vier Bausteine fügen sich zu *einem* Rust-Daemon + CLI + MCP-Server mit gemeinsamem
typisiertem Zustands-Kern:

1. **Lease-Kern** mit TTL + Heartbeat-Renewal + Stale-Reclaim (etcd/ZK-Muster) **plus monotonem
   Fencing-Token** auf jedem folgenreichen Write/Merge → Split-Brain unmöglich, auch wenn eine
   pausierte Session wieder erwacht. Single-Host vereinfacht (keine Partitionen), aber Pausen-Race
   bleibt → Fencing nötig.
2. **`notify` + `notify-debouncer-full`** als Push-Quelle auf Zustands-Ordner + SESSION-SYNC.md;
   Debouncing zwingend (Event-Verstärkung), Ordner statt nur Datei beobachten (atomic-rename).
3. **`cargo-dist`** für die macOS/Linux/Windows-Binärmatrix + Shell-Installer + SBOM; ggf.
   überdimensioniert für ein Ein-Plattform-Tool (Abwägung offen).
4. **`rmcp`** exponiert `register`/`claim`/`merge-lock`/`status` als typisierte MCP-Tools — der
   direkte Pfad von „Shell-Strings" zu „typisiertem, schema-validiertem Zustand" + Push-Notifications.

**Offene/umstrittene Punkte:** cargo-dist-Maintenance-Status vor Adoption prüfen; rmcp-API noch in
Major-Version-Bewegung (Breaking Changes); macOS-FSEvents-Ownership-Caveat im konkreten Setup
verifizieren; rename-over-Watch-Empfehlung ist Engineering-Konsens, nicht primärquellen-zitiert.
