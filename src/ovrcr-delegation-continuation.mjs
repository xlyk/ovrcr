// Lifecycle restoration is an index lookup and Server reconciliation, never an input retry.
const TYPE = "ovrcr_workflow_pointer";
const id = value => typeof value === "string" && /^[A-Za-z0-9_-]{1,128}$/.test(value);
export function validatePointer(pointer) {
  if (!pointer || typeof pointer !== "object" || Array.isArray(pointer) || Object.keys(pointer).sort().join() !== "digest,instance,schema,version,workflow" || pointer.schema !== 1 || !id(pointer.instance) || !id(pointer.workflow) || !Number.isSafeInteger(pointer.version) || pointer.version < 1 || typeof pointer.digest !== "string" || !/^[a-f0-9]{64}$/.test(pointer.digest)) throw new Error("Malformed workflow pointer; original records remain retained");
  return pointer;
}

export function createDelegationContinuation({ pi, call, auth, enabled = () => true, restored }) {
  let state = null;
  let gap = null;
  let ready = Promise.resolve();
  let generation = 0;
  let active = true;
  function pointer(ctx) {
    const entries = ctx.sessionManager.getBranch?.() ?? ctx.sessionManager.getEntries?.() ?? [];
    // Do not scan an unbounded transcript to reconstruct authority.
    if (!Array.isArray(entries) || entries.length > 65536) throw new Error("Workflow pointer search exceeds its bound; recover from the Server explicitly");
    for (let i = entries.length - 1; i >= 0; i--) {
      const entry = entries[i];
      if (entry?.type === "custom" && entry.customType === TYPE) return validatePointer(entry.data);
    }
    return null;
  }
  function remember() {
    if (state?.pointer) pi.appendEntry?.(TYPE, validatePointer(state.pointer));
  }
  async function reconcile(ctx, workflow) {
    const mine = ++generation;
    const response = await call({ Recover: { auth: auth(), workflow: workflow ?? null } });
    const rows = Array.isArray(response) ? response : response?.workflows;
    const recoveryGaps = Array.isArray(response?.gaps) ? response.gaps : [];
    if (!active || mine !== generation) throw new Error("Workflow recovery was retired by a session change");
    if (!Array.isArray(rows) || rows.length > 64) throw new Error("Workflow discovery exceeds its bound; choose an explicit workflow");
    if (rows.length > 1) throw new Error("Several workflows belong to this Pi; select one with ovrcr_recover");
    const next = rows[0] ?? null;
    const missing = recoveryGaps.length ? `Retained workflow recovery gaps: ${JSON.stringify(recoveryGaps)}` : null;
    if (next && missing) next.gaps = [...(next.gaps ?? []), missing];
    if (next && (!next.active || !Array.isArray(next.handles) || next.handles.length > 4096 || !id(next.authority?.workflow))) throw new Error("Workflow has unresolved current authority");
    if (next?.pointer) validatePointer(next.pointer);
    state = next;
    gap = missing;
    restored(next);
    remember();
    return next ?? (missing ? { recovery_gaps: recoveryGaps } : null);
  }
  function restore(event, ctx) {
    if (!enabled()) return;
    const task = async () => {
      if (["fork", "new"].includes(event.reason)) {
        state = null;
        restored(null);
        gap = "Copied or forked workflow pointers grant no control. Create a distinct workflow or request an explicit transfer.";
        return;
      }
      const saved = pointer(ctx);
      const next = await reconcile(ctx, saved?.workflow);
      if (saved && !next?.recovery_gaps && (!next || next.authority?.instance !== saved.instance)) throw new Error("Workflow pointer belongs to another instance; no work was resumed");
    };
    ready = task().catch(error => { state = null; gap = String(error.message); restored(null); });
    return ready;
  }
  pi.on("session_start", restore);
  pi.on("session_before_compact", () => { if (active) remember(); });
  pi.on("session_compact", (_event, ctx) => restore({ reason: "compaction" }, ctx));
  pi.on("session_compact_failed", (_event, ctx) => restore({ reason: "compaction" }, ctx));
  pi.on("session_shutdown", () => { active = false; generation += 1; });
  pi.on("before_agent_start", async () => {
    await ready;
    if (!state && !gap) return;
    const content = `OVRCR workflow continuation. Read original artifacts before acting; receipts and human-control barriers remain authoritative. ${JSON.stringify({ pointer: state?.pointer, authority: state?.authority, continuation: state?.continuation, handles: state?.handles, gaps: state?.gaps, recovery_gap: gap })}`;
    return { message: { customType: "ovrcr_continuation", content, display: false } };
  });
  return {
    ready: () => ready,
    get state() { return state; },
    get gap() { return gap; },
    async recover(ctx, workflow) { await ready; if (!active) throw new Error("Workflow source was retired"); return reconcile(ctx, workflow); },
    async save(index) {
      await ready;
      if (!active || !state) throw new Error("Reconcile an existing workflow before saving its continuation");
      const mine = generation;
      const rows = await call({ Save: { auth: auth(), workflow: state.authority.workflow, expected_version: state.pointer?.version ?? 0, index } });
      if (!active || generation !== mine) throw new Error("Continuation reply belongs to a retired source; reconcile original publication");
      if (!Array.isArray(rows) || rows.length !== 1) throw new Error("Invalid continuation receipt");
      const next = rows[0];
      validatePointer(next.pointer);
      state = next;
      restored(next);
      remember();
      return next;
    },
    // The first Prepare may create a workflow before there was any continuation pointer.
    async discover(ctx) { await ready; if (!state && active) return reconcile(ctx); return state; },
  };
}
