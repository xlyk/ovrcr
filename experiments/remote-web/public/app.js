const LIST_POLL_MS = 5000;
const SCREEN_POLL_MS = 1500;
const STALE_AFTER_MS = 12000;

const KEYS = [
  [":escape:", "Esc"],
  [":tab:", "Tab"],
  [":backtab:", "⇧Tab"],
  [":up:", "↑"],
  [":down:", "↓"],
  [":left:", "←"],
  [":right:", "→"],
  [":enter:", "↵"],
  [":ctrl-c:", "^C"],
  [":1:", "1"],
  [":2:", "2"],
  [":y:", "y"],
  [":n:", "n"],
];

const GROUPS = [
  ["needs", "Needs you"],
  ["working", "Working"],
  ["idle", "Idle"],
  ["ended", "Ended"],
];

const state = {
  sessions: [],
  selectedId: null,
  screen: null,
  lastOkAt: 0,
  listBusy: false,
  screenBusy: false,
  listTimer: null,
  screenTimer: null,
  toastTimer: null,
  scrollBottom: true,
};

const $ = (id) => document.getElementById(id);

function el(tag, className, text) {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text !== undefined) node.textContent = text;
  return node;
}

async function api(path, options = {}) {
  const response = await fetch(path, { cache: "no-store", ...options });
  const raw = await response.text();
  let body = null;
  try {
    body = raw ? JSON.parse(raw) : null;
  } catch {
    body = null;
  }
  if (!response.ok) {
    const message = body?.error?.message || `HTTP ${response.status}`;
    throw new Error(message);
  }
  return body;
}

function post(path, value) {
  return api(path, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(value),
  });
}

function showBanner(message) {
  $("banner-text").textContent = message;
  $("banner").hidden = false;
  $("conn").className = "conn bad";
}

function hideBanner() {
  $("banner").hidden = true;
  $("conn").className = "conn ok";
}

function toast(message) {
  const node = $("toast");
  node.textContent = message;
  node.hidden = false;
  clearTimeout(state.toastTimer);
  state.toastTimer = setTimeout(() => {
    node.hidden = true;
  }, 2200);
}

function activityWords(session) {
  switch (session.activity) {
    case "waiting_input":
      return "needs input";
    case "busy":
      return "working";
    case "idle":
      return "idle";
    case "response_ready":
      return "ready";
    case "error":
      return "error";
    default:
      return "—";
  }
}

function glyph(session, group) {
  if (session.unread && group !== "working") return ["✓", "ready"];
  if (group === "needs") return ["?", "needs"];
  if (group === "working") return ["●", "working"];
  if (group === "ended") return ["×", "ended"];
  return ["○", "idle"];
}

function groupOf(session) {
  if (session.archived) return null;
  if (session.activity === "waiting_input") return "needs";
  if (session.unread && session.phase === "running" && session.activity !== "busy") {
    return "needs";
  }
  if (session.phase === "running" && session.activity === "busy") return "working";
  if (session.phase === "running" || session.phase === "paused") return "idle";
  return "ended";
}

function titleOf(session) {
  return session.title || session.display_name || session.name || `session ${session.id}`;
}

function placeOf(session) {
  const parts = [session.project, session.workspace].filter(Boolean);
  if (parts.length) return parts.join(" / ");
  if (session.scope && session.scope.OperatorConversation) return "operator";
  return session.cwd || "";
}

function elapsed(session) {
  if (!session.started_unix_ms) return "";
  const ms = Date.now() - session.started_unix_ms;
  if (ms < 0) return "";
  const minutes = Math.floor(ms / 60000);
  if (minutes < 1) return "just now";
  if (minutes < 60) return `${minutes}m`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours}h ${minutes % 60}m`;
  return `${Math.floor(hours / 24)}d`;
}

function detailLine(session) {
  const bits = [placeOf(session), activityWords(session), elapsed(session)].filter(Boolean);
  if (session.unread) bits.push("ready to review");
  return bits.join(" · ");
}

function providerLabel(session) {
  return session.label || session.agent?.binding?.provider || "";
}

function hasTerminal(session) {
  return Boolean(session) && (session.phase === "running" || session.phase === "paused");
}

function renderList() {
  const groups = $("groups");
  groups.replaceChildren();

  const visible = state.sessions.filter((session) => groupOf(session) !== null);
  $("empty").hidden = visible.length > 0;

  for (const [key, label] of GROUPS) {
    const members = visible.filter((session) => groupOf(session) === key);
    if (members.length === 0) continue;

    groups.append(el("h2", "group-title", `${label} ${members.length}`));
    const list = el("ul", "rows");

    for (const session of members) {
      const [symbol, tone] = glyph(session, key);
      const row = el("li", "session");
      row.dataset.id = session.id;
      row.append(el("span", `glyph ${tone}`, symbol));

      const main = el("div", "row-main");
      main.append(el("div", "row-title", titleOf(session)));
      main.append(el("div", "row-sub", detailLine(session)));
      row.append(main);

      const provider = providerLabel(session);
      if (provider) row.append(el("span", "row-agent", provider));

      row.addEventListener("click", () => select(session.id));
      list.append(row);
    }

    groups.append(list);
  }
}

function renderSessionHeader() {
  const session = state.sessions.find((item) => item.id === state.selectedId);
  const title = session ? titleOf(session) : `session ${state.selectedId}`;
  $("title").textContent = title;
  $("subtitle").textContent = session ? detailLine(session) : "";
}

function renderMeta() {
  const session = state.sessions.find((item) => item.id === state.selectedId);
  const meta = $("meta");
  meta.replaceChildren();
  if (!session) return;

  const bits = [
    `${placeOf(session) || "unknown"}`,
    `#${session.id}`,
    session.run !== undefined ? `run ${session.run}` : "",
    session.phase,
    activityWords(session),
    session.label || "",
  ].filter(Boolean);

  for (const bit of bits) meta.append(el("span", "", bit));
}

function renderActions() {
  const session = state.sessions.find((item) => item.id === state.selectedId);
  const actions = $("actions");
  actions.replaceChildren();
  if (!session) return;

  if (session.phase === "running") {
    actions.append(actionButton("Pause", () => mutate("pause", "paused")));
  } else if (session.phase === "paused") {
    actions.append(actionButton("Resume", () => mutate("resume", "resumed")));
  }

  if (session.unread) {
    actions.append(
      actionButton("Review", async () => {
        await post(`/api/session/${session.id}/review`, { expected: session.unread });
        toast("marked reviewed");
        await refreshList();
      }),
    );
  }

  actions.append(actionButton("Reload", () => refreshScreen()));
  actions.append(
    actionButton(
      "Kill",
      async () => {
        if (!confirm(`Kill session #${session.id}?`)) return;
        await mutate("kill", "kill requested");
        setTimeout(() => back(), 400);
      },
      "danger",
    ),
  );
}

function actionButton(label, onClick, extra = "") {
  const button = el("button", extra, label);
  button.type = "button";
  button.addEventListener("click", async () => {
    button.disabled = true;
    try {
      await onClick();
    } catch (error) {
      showBanner(error.message);
    } finally {
      button.disabled = false;
    }
  });
  return button;
}

async function mutate(action, message) {
  await post(`/api/session/${state.selectedId}/${action}`, {});
  toast(message);
  await refreshList();
}

function renderComposerState() {
  const session = state.sessions.find((item) => item.id === state.selectedId);
  const running = session?.phase === "running";
  $("text").disabled = !running;
  $("send").disabled = !running;
  $("type-only").disabled = !running;
  $("text").placeholder = running
    ? "Reply to this session…"
    : session
      ? `Session is ${session.phase}`
      : "Select a session";
}

function renderScreen() {
  const screen = $("screen");
  const session = state.sessions.find((item) => item.id === state.selectedId);
  if (session && !hasTerminal(session)) {
    screen.textContent =
      `This session is ${session.phase}; it has no live terminal.\n\n` +
      `Reopen it from the desktop with:\n  ovrcr terminal reopen ${session.id}`;
    return;
  }
  const text = state.screen?.text ?? "";
  screen.textContent = text;
  if (state.scrollBottom) screen.scrollTop = screen.scrollHeight;
}

function renderSession() {
  renderSessionHeader();
  renderMeta();
  renderActions();
  renderComposerState();
  renderScreen();
}

async function refreshList() {
  if (state.listBusy) return;
  state.listBusy = true;
  try {
    const data = await api("/api/state");
    state.sessions = data.sessions || [];
    state.lastOkAt = Date.now();
    hideBanner();
    $("subtitle").textContent = `${state.sessions.length} session${
      state.sessions.length === 1 ? "" : "s"
    }`;
    renderList();
    if (state.selectedId) {
      renderSessionHeader();
      renderMeta();
      renderActions();
      renderComposerState();
      renderScreen();
    }
  } catch (error) {
    if (Date.now() - state.lastOkAt > STALE_AFTER_MS) showBanner(error.message);
  } finally {
    state.listBusy = false;
  }
}

async function refreshScreen() {
  if (state.screenBusy || !state.selectedId) return;
  const current = state.sessions.find((item) => item.id === state.selectedId);
  if (current && !hasTerminal(current)) return;
  state.screenBusy = true;
  const id = state.selectedId;
  try {
    const data = await api(`/api/session/${id}/screen?lines=40`);
    if (state.selectedId !== id) return;
    state.screen = data;
    state.lastOkAt = Date.now();
    hideBanner();
    renderScreen();
  } catch (error) {
    if (state.selectedId === id) showBanner(error.message);
  } finally {
    state.screenBusy = false;
  }
}

function syncPolling() {
  clearInterval(state.listTimer);
  clearInterval(state.screenTimer);
  state.listTimer = null;
  state.screenTimer = null;
  if (document.hidden) return;

  state.listTimer = setInterval(refreshList, LIST_POLL_MS);
  if (state.selectedId) {
    state.screenTimer = setInterval(refreshScreen, SCREEN_POLL_MS);
  }
}

function syncConn() {
  const fresh = Date.now() - state.lastOkAt < STALE_AFTER_MS;
  if ($("banner").hidden) {
    $("conn").className = fresh ? "conn ok" : "conn";
  }
}

async function select(id, { push = true } = {}) {
  state.selectedId = id;
  state.screen = null;
  state.scrollBottom = true;
  if (push && location.hash !== `#/s/${id}`) location.hash = `#/s/${id}`;
  $("list-view").hidden = true;
  $("session-view").hidden = false;
  $("back").hidden = false;
  renderSession();
  syncPolling();
  await refreshList();
  if (hasTerminal(state.sessions.find((item) => item.id === state.selectedId))) {
    await refreshScreen();
  }
  renderSession();
}

function back({ push = true } = {}) {
  state.selectedId = null;
  state.screen = null;
  if (push && location.hash) location.hash = "";
  $("session-view").hidden = true;
  $("list-view").hidden = false;
  $("back").hidden = true;
  $("title").textContent = "OVRCR";
  $("subtitle").textContent = `${state.sessions.length} sessions`;
  syncPolling();
}

async function sendText(submit) {
  if (!state.selectedId) return;
  const box = $("text");
  const text = box.value;
  if (!text && !submit) return;
  try {
    await post(`/api/session/${state.selectedId}/send`, { text, submit });
    toast(submit ? "sent" : "typed");
    if (submit) box.value = "";
    setTimeout(refreshScreen, 250);
  } catch (error) {
    showBanner(error.message);
  }
}

async function sendKey(key) {
  if (!state.selectedId) return;
  try {
    await post(`/api/session/${state.selectedId}/key`, { key });
    setTimeout(refreshScreen, 200);
  } catch (error) {
    showBanner(error.message);
  }
}

function buildKeybar() {
  const bar = $("keybar");
  for (const [key, label] of KEYS) {
    const button = el("button", "", label);
    button.type = "button";
    button.addEventListener("click", () => sendKey(key));
    bar.append(button);
  }
}

function route() {
  const match = location.hash.match(/^#\/s\/([1-9][0-9]*)$/);
  if (match) {
    select(Number(match[1]), { push: false });
  } else if (state.selectedId !== null) {
    back({ push: false });
  }
}

function wire() {
  $("back").addEventListener("click", () => back());
  $("banner-dismiss").addEventListener("click", hideBanner);
  $("composer").addEventListener("submit", (event) => {
    event.preventDefault();
    sendText(true);
  });
  $("type-only").addEventListener("click", () => sendText(false));
  $("text").addEventListener("keydown", (event) => {
    if ((event.metaKey || event.ctrlKey) && event.key === "Enter") {
      event.preventDefault();
      sendText(true);
    }
  });
  $("screen").addEventListener("scroll", () => {
    const node = $("screen");
    state.scrollBottom = node.scrollHeight - node.scrollTop - node.clientHeight < 48;
  });
  document.addEventListener("visibilitychange", () => {
    syncPolling();
    if (!document.hidden) {
      refreshList();
      if (state.selectedId) refreshScreen();
    }
  });
  window.addEventListener("hashchange", route);
  setInterval(syncConn, 1000);

  buildKeybar();
  route();
  if (state.selectedId === null) {
    $("subtitle").textContent = "";
    refreshList();
  }
  syncPolling();
}

wire();
