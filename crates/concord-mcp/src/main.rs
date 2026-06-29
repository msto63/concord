//! concord-mcp — the Concord MCP server (M3-lean / WP9-lean).
//!
//! Exposes ONLY Concord's *enforced* coordination primitives as typed MCP tools, so an
//! autonomous coordinator (the `hub` agent) drives the core schema-validated instead of
//! via bash strings. This is deliberately NOT a broad coordination surface (that is a
//! 2026 commodity — see ADR-0002); it is the small set of operations whose value is
//! their *enforcement*: ownership-checked release/unlock, the fencing token, and the
//! airtight daemon-mediated merge-lock.
//!
//! One typed core (`concord-core`), three surfaces: CLI (M1), push daemon (M2), and this
//! MCP server (M3-lean). The tokio/rmcp dependency is isolated to this crate so the core,
//! CLI, and daemon stay std-only.
//!
//! Transport: stdio (what an MCP host such as Claude Code expects). Consequential merge
//! ops route through the M2 daemon socket when it is up (airtight), else the Floor.

use concord_core::ipc::{self, Request, Response, SOCKET_NAME};
use concord_core::store::{
    ClaimOutcome, HoldStatus, MergeLockOutcome, MergeUnlockOutcome, OverlapPolicy, ReleaseOutcome,
    StatusReport,
};
use concord_core::{Paths, Store};

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, Content};
use rmcp::{tool, tool_handler, tool_router, ServerHandler, ServiceExt};
use schemars::JsonSchema;
use serde::Deserialize;

// ───────────────────────── tool argument schemas ─────────────────────────

#[derive(Debug, Deserialize, JsonSchema)]
struct RegisterArgs {
    /// The session id to register (e.g. "hub", "a").
    id: String,
    /// A short focus description (optional).
    #[serde(default)]
    focus: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct IdArgs {
    /// The session id.
    id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct ClaimArgs {
    /// The claiming session id.
    id: String,
    /// The area / path / region to lease (path-prefix overlap is rejected).
    area: String,
    /// Why the lease is taken (optional).
    #[serde(default)]
    why: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct ReleaseArgs {
    id: String,
    area: String,
    /// If set, refuse the release unless the lease still carries this fence (fencing Floor).
    #[serde(default)]
    fence: Option<u64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct AreaArgs {
    id: String,
    area: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct MergeLockArgs {
    id: String,
    #[serde(default)]
    why: String,
}

// ───────────────────────────── the server ─────────────────────────────

#[derive(Clone)]
struct ConcordServer {
    tool_router: ToolRouter<ConcordServer>,
}

#[tool_router]
impl ConcordServer {
    fn new() -> Self {
        Self {
            tool_router: Self::tool_router(),
        }
    }

    /// Open a fresh store (fresh timestamp) rooted at the server's working directory —
    /// the same env-based resolution the CLI and daemon use.
    fn store(&self) -> Result<Store, String> {
        Store::open(Paths::from_cwd()).map_err(|e| e.to_string())
    }

    #[tool(description = "Register a session in the coordination registry (id + focus).")]
    async fn register(&self, Parameters(a): Parameters<RegisterArgs>) -> CallToolResult {
        match self.store() {
            Ok(s) => match s.register(&a.id, &a.focus) {
                Ok(_) => text(format!("registered session '{}' (focus: {})", a.id, a.focus)),
                Err(e) => err(e.to_string()),
            },
            Err(e) => err(e),
        }
    }

    #[tool(description = "Refresh a session's heartbeat (keeps it non-stale).")]
    async fn heartbeat(&self, Parameters(a): Parameters<IdArgs>) -> CallToolResult {
        match self
            .store()
            .and_then(|s| s.heartbeat(&a.id).map_err(|e| e.to_string()))
        {
            Ok(()) => text(format!("heartbeat {}", a.id)),
            Err(e) => err(e),
        }
    }

    #[tool(description = "Show active sessions, held leases, and the merge-lock holder.")]
    async fn status(&self) -> CallToolResult {
        let store = match self.store() {
            Ok(s) => s,
            Err(e) => return err(e),
        };
        match store.status() {
            Ok(r) => text(render_status(&r, &store)),
            Err(e) => err(e.to_string()),
        }
    }

    #[tool(
        description = "Claim a lease on a shared area. Rejects a path-prefix overlap with a live lease (enforced §6)."
    )]
    async fn claim(&self, Parameters(a): Parameters<ClaimArgs>) -> CallToolResult {
        // Strong tier: airtight daemon-mediated claim when the daemon is up.
        if let Some(resp) = self.mediate(Request::Claim {
            id: a.id.clone(),
            area: a.area.clone(),
            why: a.why.clone(),
        }) {
            return match resp {
                Response::Claimed => text(format!("CLAIMED {}", a.area)),
                Response::AlreadyYours => text(format!("already yours: {}", a.area)),
                Response::Reclaimed { previous } => {
                    text(format!("RECLAIMED {} (stale holder {previous})", a.area))
                }
                Response::ClaimConflict { holder } => {
                    text(format!("CONFLICT: '{}' is leased by '{holder}'", a.area))
                }
                Response::Overlap { area, holder } => text(format!(
                    "OVERLAP: '{}' path-overlaps '{area}' leased by '{holder}'",
                    a.area
                )),
                _ => err("unexpected daemon response".to_string()),
            };
        }
        let store = match self.store() {
            Ok(s) => s,
            Err(e) => return err(e),
        };
        match store.claim(&a.id, &a.area, &a.why, OverlapPolicy::RejectOverlap) {
            Ok(ClaimOutcome::Claimed) => text(format!("CLAIMED {}", a.area)),
            Ok(ClaimOutcome::AlreadyYours) => text(format!("already yours: {}", a.area)),
            Ok(ClaimOutcome::Reclaimed { previous }) => {
                text(format!("RECLAIMED {} (stale holder {previous})", a.area))
            }
            Ok(ClaimOutcome::Conflict { holder }) => {
                text(format!("CONFLICT: '{}' is leased by '{holder}'", a.area))
            }
            Ok(ClaimOutcome::OverlapConflict { area, holder }) => text(format!(
                "OVERLAP: '{}' path-overlaps '{area}' leased by '{holder}'",
                a.area
            )),
            Err(e) => err(e.to_string()),
        }
    }

    #[tool(
        description = "Release a lease. Refuses to release a foreign lease (ownership-enforced); with `fence`, refuses if the lease's fence advanced (fencing Floor)."
    )]
    async fn release(&self, Parameters(a): Parameters<ReleaseArgs>) -> CallToolResult {
        if let Some(resp) = self.mediate(Request::Release {
            id: a.id.clone(),
            area: a.area.clone(),
            fence: a.fence,
        }) {
            return match resp {
                Response::Released => text(format!("released {}", a.area)),
                Response::NoLease => text(format!("no lease on {}", a.area)),
                Response::NotYours { holder } => text(format!(
                    "REFUSED: '{}' held by '{holder}', not '{}'",
                    a.area, a.id
                )),
                Response::FenceStale { current } => text(format!(
                    "REFUSED: '{}' fence advanced to {current} (your authority is stale)",
                    a.area
                )),
                _ => err("unexpected daemon response".to_string()),
            };
        }
        let store = match self.store() {
            Ok(s) => s,
            Err(e) => return err(e),
        };
        match store.release(&a.id, &a.area, a.fence) {
            Ok(ReleaseOutcome::Released) => text(format!("released {}", a.area)),
            Ok(ReleaseOutcome::NoLease) => text(format!("no lease on {}", a.area)),
            Ok(ReleaseOutcome::NotYours { holder }) => text(format!(
                "REFUSED: '{}' held by '{holder}', not '{}'",
                a.area, a.id
            )),
            Ok(ReleaseOutcome::FenceStale { current }) => text(format!(
                "REFUSED: '{}' fence advanced to {current} (your authority is stale)",
                a.area
            )),
            Err(e) => err(e.to_string()),
        }
    }

    #[tool(
        description = "Verify whether a session still legitimately holds an area (fence-aware self-check before acting)."
    )]
    async fn verify(&self, Parameters(a): Parameters<AreaArgs>) -> CallToolResult {
        let store = match self.store() {
            Ok(s) => s,
            Err(e) => return err(e),
        };
        match store.verify_hold(&a.id, &a.area) {
            Ok(HoldStatus::Held { fence }) => text(format!("HELD by {} (fence {fence})", a.id)),
            Ok(HoldStatus::HeldByOther { holder }) => text(format!("HELD-BY-OTHER {holder}")),
            Ok(HoldStatus::Stale { holder }) => text(format!("STALE (was {holder}, reclaimable)")),
            Ok(HoldStatus::Vacant) => text("VACANT".to_string()),
            Err(e) => err(e.to_string()),
        }
    }

    #[tool(
        description = "Acquire the singleton merge lock (routes through the daemon for an airtight check-and-apply when it is up, else the Floor)."
    )]
    async fn merge_lock(&self, Parameters(a): Parameters<MergeLockArgs>) -> CallToolResult {
        // Strong tier first: mediate through the daemon socket when present.
        if let Some(resp) = self.mediate(Request::MergeLock {
            id: a.id.clone(),
            why: a.why.clone(),
        }) {
            return match resp {
                Response::Acquired { .. } => text("MERGE LOCK acquired".to_string()),
                Response::Reacquired { .. } => text("MERGE LOCK (re)acquired".to_string()),
                Response::Held { holder } => {
                    text(format!("MERGE LOCK held by '{holder}' — wait until released"))
                }
                _ => err("unexpected daemon response".to_string()),
            };
        }
        let store = match self.store() {
            Ok(s) => s,
            Err(e) => return err(e),
        };
        match store.merge_lock(&a.id, &a.why) {
            Ok(MergeLockOutcome::Acquired) => text("MERGE LOCK acquired".to_string()),
            Ok(MergeLockOutcome::Reacquired) => text("MERGE LOCK (re)acquired".to_string()),
            Ok(MergeLockOutcome::Held { holder }) => {
                text(format!("MERGE LOCK held by '{holder}' — wait until released"))
            }
            Err(e) => err(e.to_string()),
        }
    }

    #[tool(
        description = "Release the merge lock (daemon-mediated when up; refuses to unlock a lock held by another session)."
    )]
    async fn merge_unlock(&self, Parameters(a): Parameters<IdArgs>) -> CallToolResult {
        if let Some(resp) = self.mediate(Request::MergeUnlock { id: a.id.clone() }) {
            return match resp {
                Response::Released => text("merge lock released".to_string()),
                Response::NotHeld => text("merge lock not held".to_string()),
                Response::NotYours { holder } => text(format!(
                    "REFUSED: merge lock held by '{holder}', not '{}'",
                    a.id
                )),
                _ => err("unexpected daemon response".to_string()),
            };
        }
        let store = match self.store() {
            Ok(s) => s,
            Err(e) => return err(e),
        };
        match store.merge_unlock(&a.id) {
            Ok(MergeUnlockOutcome::Released) => text("merge lock released".to_string()),
            Ok(MergeUnlockOutcome::NotHeld) => text("merge lock not held".to_string()),
            Ok(MergeUnlockOutcome::NotYours { holder }) => text(format!(
                "REFUSED: merge lock held by '{holder}', not '{}'",
                a.id
            )),
            Err(e) => err(e.to_string()),
        }
    }

    /// Try the daemon mediation socket (Strong tier). `None` ⇒ no daemon ⇒ Floor.
    fn mediate(&self, req: Request) -> Option<Response> {
        let paths = Paths::from_cwd();
        ipc::mediate(&paths.coord.join(SOCKET_NAME), &req)
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for ConcordServer {}

// ───────────────────────────── helpers ─────────────────────────────

fn text(s: String) -> CallToolResult {
    CallToolResult::success(vec![Content::text(s)])
}

/// An error outcome surfaced as tool content (the coordinator reads the message).
fn err(s: String) -> CallToolResult {
    CallToolResult::success(vec![Content::text(format!("ERROR: {s}"))])
}

fn render_status(r: &StatusReport, store: &Store) -> String {
    let mut out = String::new();
    out.push_str(&format!("coord: {}\n", store.paths().coord.display()));
    out.push_str("sessions:\n");
    if r.sessions_dir_empty {
        out.push_str("  (none)\n");
    } else {
        for s in &r.sessions {
            out.push_str(&format!("  {:<10} {}\n", s.id, s.focus));
        }
    }
    out.push_str("leases:\n");
    if r.leases.is_empty() {
        out.push_str("  (none)\n");
    } else {
        for l in &r.leases {
            out.push_str(&format!(
                "  {:<28} by {} (fence {}) — {}\n",
                l.area, l.holder, l.fence, l.why
            ));
        }
    }
    if let Some(h) = &r.merge_lock_holder {
        out.push_str(&format!("merge-lock: {h}\n"));
    }
    out
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // stdio MCP server: the host (e.g. Claude Code) spawns this process and speaks MCP
    // over stdin/stdout. Logs must go to stderr only (stdout is the protocol channel).
    let service = ConcordServer::new().serve(rmcp::transport::stdio()).await?;
    service.waiting().await?;
    Ok(())
}
