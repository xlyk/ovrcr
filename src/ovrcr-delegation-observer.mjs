// One bounded control observer per managed Pi extension. Files/tools carry content;
// continuations carry only identities and observation discriminants.
const BATCH = 64;
const TRACKED = 4096;
const pause = (ms, signal) => new Promise(resolve => {
  const done = () => { clearTimeout(timer); signal.removeEventListener("abort", done); resolve(); };
  const timer = setTimeout(done, ms);
  signal.addEventListener("abort", done, { once: true });
  if (signal.aborted) done();
});

export function createDelegationObserver({ pi, call, auth }) {
  const handles = new Set();
  const seen = new Map();
  const pending = new Set();
  let cursor;
  let controller;
  let generation = 0;
  let running = false;
  let active = true;
  let queued = false;
  let disconnected = false;
  let offset = 0;

  function deliver(status = "changed") {
    if (!active || queued || !pending.size) return;
    const ids = [...pending].slice(0, BATCH);
    const metadata = { status, handles: ids, reconcile: true };
    try {
      pi.sendMessage({
        customType: "ovrcr_wake",
        content: `OVRCR assignment observations changed. Reconcile outstanding assignments with ovrcr_wait; collect available files separately. A wake does not accept work or confirm returned control. ${JSON.stringify(metadata)}`,
        display: false,
        details: metadata,
      }, { triggerTurn: true, deliverAs: "followUp" });
      queued = true;
      for (const id of ids) pending.delete(id);
    } catch {
      // Leave the bounded pending set intact; the next observation retries delivery.
    }
  }

  function observe(batch) {
    for (const item of batch.observations) {
      const facts = {};
      if (item.publication && !item.publication.handled) facts.publication = item.publication.identity;
      if (item.input_requests.length) facts.input = { requests: item.input_requests, revision: item.input_revision };
      if (!item.publication?.handled && item.activity?.state === "ResponseReady") facts.ready = { activity: item.activity, revision: item.activity_revision };
      if (item.stalled) facts.stalled = true;
      if (item.publication_issue) facts.publication_issue = item.publication_issue;
      if (!item.current) facts.current = false;
      if (item.control && (item.control.owner !== "Controller" || item.control.barrier || item.control.attribution_invalidated)) {
        const { owner, epoch, barrier, attribution_invalidated } = item.control;
        facts.control = { owner, epoch, barrier, attribution_invalidated };
      }
      const signal = Object.keys(facts).length ? facts : null;
      const key = JSON.stringify(signal);
      if (seen.get(item.handle) !== key && signal) pending.add(item.handle);
      if (!signal) pending.delete(item.handle);
      seen.set(item.handle, key);
    }
    deliver();
  }

  function start() {
    if (running || !active || !handles.size) return;
    running = true;
    controller = new AbortController();
    const signal = controller.signal;
    const mine = ++generation;
    void (async () => {
      while (active && mine === generation && !signal.aborted) {
        const ids = [...handles];
        const selected = ids.slice(offset, offset + BATCH);
        offset = offset + BATCH >= ids.length ? 0 : offset + BATCH;
        try {
          const batch = await call({ Observe: { auth: auth(), handles: selected, after: cursor, wait_ms: ids.length > BATCH ? 0 : 1000 } }, { signal });
          if (!active || mine !== generation) break;
          // Several batches share a ring cursor; reconcile each batch independently.
          cursor = ids.length > BATCH ? undefined : batch.cursor;
          disconnected = false;
          observe(batch);
        } catch {
          if (signal.aborted || !active || mine !== generation) break;
          if (!disconnected) {
            disconnected = true;
            for (const id of selected) pending.add(id);
            deliver("disconnected");
          }
          cursor = undefined;
          await pause(1000, signal);
        }
        // Prevent a refused/instant response or many batches from spinning.
        await pause(25, signal);
      }
    })().finally(() => { if (mine === generation) running = false; });
  }

  return {
    watch(handle) {
      if (!handles.has(handle) && handles.size === TRACKED) throw new Error("observation tracking capacity exhausted");
      handles.add(handle);
      cursor = undefined;
      start();
    },
    acknowledge() { queued = false; deliver(); },
    async wait(args, signal) {
      const ids = args.handles ?? [...handles].slice(0, BATCH);
      if (!ids.length || ids.length > BATCH) throw new Error("wait requires 1..64 handles");
      const batch = await call({ Observe: { auth: auth(), handles: ids, after: args.after ?? null, wait_ms: args.wait_ms ?? 1000 } }, { signal });
      for (const id of ids) {
        if (!handles.has(id) && handles.size === TRACKED) throw new Error("observation tracking capacity exhausted");
        handles.add(id);
      }
      start();
      return batch;
    },
    reconnect() {
      generation += 1;
      controller?.abort();
      running = false;
      cursor = undefined;
      queued = false;
      seen.clear();
      start();
    },
    stop() { active = false; generation += 1; controller?.abort(); },
  };
}
