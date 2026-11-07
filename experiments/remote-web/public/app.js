const LIST_POLL_MS = 5000;
const SCREEN_POLL_MS = 1500;
const STALE_AFTER_MS = 12000;
// JetBrains Mono Nerd Font cell height is (1020 + 300) / 1000 = 1.32em;
// at 12px that is 15.84px. 16px keeps block and Powerline cells tiled.
const ROW_PX = 16;
const TAIL_ROWS = 40;
const PAGE_ROWS = 40;

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

const BASE16 = [
  "#000000", "#cd0000", "#00cd00", "#cdcd00", "#0000ee", "#cd00cd", "#00cdcd", "#e5e5e5",
  "#7f7f7f", "#ff0000", "#00ff00", "#ffff00", "#5c5cff", "#ff00ff", "#00ffff", "#ffffff",
];
const CUBE = [0, 95, 135, 175, 215, 255];
const DEFAULT_FG = "#cfd6e4";
const DEFAULT_BG = "#0b0d11";

const state = {
  sessions: [],
  projects: [],
  agents: [],
  newBusy: false,
  selectedId: null,
  view: "screen",
  window: null,
  rows: new Map(),
  transcript: null,
  loadingOlder: false,
  lastOkAt: 0,
  listBusy: false,
  listPromise: null,
  screenBusy: false,
  listTimer: null,
  screenTimer: null,
  toastTimer: null,
  pinned: true,
  programmaticTop: null,
  scrollFrame: null,
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
    throw new Error(body?.error?.message || `HTTP ${response.status}`);
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

function workspaceName(projectName, workspaceId) {
  const workspace = projectRecord(projectName)?.workspaces?.find((item) => item.id === workspaceId);
  return workspace?.branch || workspace?.path?.split("/").filter(Boolean).pop() || workspaceId;
}

function placeOf(session) {
  if (session.project) {
    const workspace = session.workspace ? workspaceName(session.project, session.workspace) : "";
    return [session.project, workspace].filter(Boolean).join(" / ");
  }
  if (session.workspace) return workspaceName("", session.workspace);
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
  const kindAgent = session.kind?.Agent?.name;
  return session.label || kindAgent || session.agent?.binding?.provider || "";
}

function hasTerminal(session) {
  return Boolean(session) && (session.phase === "running" || session.phase === "paused");
}

function currentSession() {
  return state.sessions.find((item) => item.id === state.selectedId);
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
  const session = currentSession();
  $("title").textContent = session ? titleOf(session) : `session ${state.selectedId}`;
  $("subtitle").textContent = session ? detailLine(session) : "";
}

function renderMeta() {
  const session = currentSession();
  const meta = $("meta");
  meta.replaceChildren();
  if (!session) return;

  const bits = [
    placeOf(session) || "unknown",
    `#${session.id}`,
    session.run !== undefined ? `run ${session.run}` : "",
    session.phase,
    activityWords(session),
    session.label || "",
  ].filter(Boolean);

  for (const bit of bits) meta.append(el("span", "", bit));
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

function renderActions() {
  const session = currentSession();
  const actions = $("actions");
  actions.replaceChildren();
  if (!session) return;

  if (hasTerminal(session)) {
    actions.append(
      actionButton(state.view === "screen" ? "Chat" : "Screen", () => toggleView()),
    );
  }

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

  actions.append(actionButton("Reload", () => (state.view === "screen" ? openScreen() : loadTranscript())));
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

function renderComposerState() {
  const session = currentSession();
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

function palette(index) {
  if (index < 16) return BASE16[index];
  if (index >= 232) {
    const value = 8 + (index - 232) * 10;
    return `rgb(${value},${value},${value})`;
  }
  const value = index - 16;
  const red = CUBE[Math.floor(value / 36) % 6];
  const green = CUBE[Math.floor(value / 6) % 6];
  const blue = CUBE[value % 6];
  return `rgb(${red},${green},${blue})`;
}

function colorOf(color) {
  if (!color || color === "Default") return null;
  if (typeof color.Indexed === "number") return palette(color.Indexed);
  if (Array.isArray(color.Rgb)) return `rgb(${color.Rgb[0]},${color.Rgb[1]},${color.Rgb[2]})`;
  return null;
}

function styleFor(cell) {
  let fg = colorOf(cell.fg) || DEFAULT_FG;
  let bg = colorOf(cell.bg) || DEFAULT_BG;
  const attributes = cell.attributes || 0;
  if (attributes & 16) {
    [fg, bg] = [bg, fg];
  }
  let css = `color:${fg};`;
  if (bg !== DEFAULT_BG) css += `background-color:${bg};`;
  if (attributes & 1) css += "font-weight:600;";
  if (attributes & 2) css += "opacity:.65;";
  if (attributes & 4) css += "font-style:italic;";
  if (attributes & 8) css += "text-decoration:underline;";
  return css;
}

function buildTermRow(row, absoluteRow) {
  const node = el("div", "term-row");
  node.style.top = `${absoluteRow * ROW_PX}px`;
  node.style.height = `${ROW_PX}px`;
  node.style.lineHeight = `${ROW_PX}px`;
  let span = null;
  let style = null;
  for (const cell of row.cells) {
    if (cell.width === 0) continue;
    const next = styleFor(cell);
    if (!span || next !== style) {
      span = document.createElement("span");
      span.style.cssText = next;
      node.append(span);
      style = next;
    }
    span.textContent += cell.text || " ";
  }
  return node;
}

function mergeWindow(data) {
  if (state.window && data.history_rows < state.window.history_rows) {
    state.rows.clear();
  }
  state.window = data;
  for (let index = 0; index < data.rows.length; index += 1) {
    state.rows.set(data.start_row + index, data.rows[index]);
  }
  for (const key of [...state.rows.keys()]) {
    if (key >= data.total_rows) state.rows.delete(key);
  }
}

function renderTerminal() {
  const screen = $("screen");
  const rows = $("screen-rows");
  const session = currentSession();

  if (!session) return;

  if (!hasTerminal(session)) {
    rows.style.height = "auto";
    rows.replaceChildren(
      el(
        "pre",
        "term-note",
        `This session is ${session.phase}; it has no live terminal.\n\n` +
          `Reopen it from the desktop with:\n  ovrcr terminal reopen ${session.id}`,
      ),
    );
    return;
  }

  const window_ = state.window;
  if (!window_) {
    rows.style.height = "auto";
    rows.replaceChildren(el("pre", "term-note", "Loading screen…"));
    return;
  }

  rows.style.height = `${window_.total_rows * ROW_PX}px`;
  if (state.pinned) scrollToCursor();
  const top = screen.scrollTop;
  const height = screen.clientHeight || 640;
  const all = window_.total_rows <= 128;
  const first = all ? 0 : Math.max(0, Math.floor(top / ROW_PX) - 2);
  const last = all
    ? window_.total_rows
    : Math.min(window_.total_rows, Math.ceil((top + height) / ROW_PX) + 2);

  rows.replaceChildren();
  for (let row = first; row < last; row += 1) {
    const data = state.rows.get(row);
    if (data) rows.append(buildTermRow(data, row));
  }

  if (window_.cursor && !window_.cursor.hidden) {
    const absolute = window_.history_rows + window_.cursor.row;
    if (absolute >= first && absolute < last) {
      const cursor = el("div", "cursor");
      cursor.style.top = `${absolute * ROW_PX}px`;
      cursor.style.height = `${ROW_PX}px`;
      cursor.style.left = `calc(12px + ${window_.cursor.col}ch)`;
      rows.append(cursor);
    }
  }
}

function renderTranscript() {
  const box = $("transcript-entries");
  box.replaceChildren();
  const transcript = state.transcript;
  if (!transcript) {
    box.append(el("p", "empty", "Loading transcript…"));
    $("load-older").hidden = true;
    return;
  }
  if (transcript.entries.length === 0) {
    box.append(el("p", "empty", "No transcript for this session."));
  }

  for (const entry of transcript.entries) {
    if (entry.User !== undefined) {
      const node = el("div", "t-entry t-user");
      node.append(el("span", "t-role", "You"));
      node.append(el("pre", "t-text", entry.User));
      box.append(node);
    } else if (entry.Assistant !== undefined) {
      const node = el("div", "t-entry t-assistant");
      node.append(el("span", "t-role", "Assistant"));
      node.append(el("pre", "t-text", entry.Assistant));
      box.append(node);
    } else if (entry.Tool) {
      const node = el("div", "t-entry t-tool");
      const name = el("span", "t-tool-name", entry.Tool.name);
      node.append(name, document.createTextNode(` ${entry.Tool.input}`));
      box.append(node);
    }
  }

  $("load-older").hidden = !(transcript.start > 0);
}

function renderSession() {
  renderSessionHeader();
  renderMeta();
  renderActions();
  renderComposerState();
  if (state.view === "screen") renderTerminal();
  else renderTranscript();
}

function scrollToCursor() {
  const screen = $("screen");
  const window_ = state.window;
  if (!window_) return;
  const cursorRow =
    window_.cursor && !window_.cursor.hidden
      ? window_.history_rows + window_.cursor.row
      : window_.total_rows - 1;
  const visible = screen.clientHeight || 640;
  const bottom = cursorRow * ROW_PX + ROW_PX;
  const maxTop = Math.max(0, screen.scrollHeight - visible);
  screen.scrollTop = Math.min(maxTop, Math.max(0, bottom - visible + ROW_PX * 3));
  state.programmaticTop = screen.scrollTop;
}

function atCursor() {
  const window_ = state.window;
  if (!window_) return true;
  const screen = $("screen");
  const cursorRow =
    window_.cursor && !window_.cursor.hidden
      ? window_.history_rows + window_.cursor.row
      : window_.total_rows - 1;
  const top = screen.scrollTop;
  const visible = screen.clientHeight || 640;
  const y = cursorRow * ROW_PX;
  return y + ROW_PX > top && y < top + visible;
}

function tailCount(total) {
  const screen = $("screen");
  const visible = Math.ceil((screen.clientHeight || 640) / ROW_PX) + 4;
  return Math.min(total, Math.min(128, Math.max(TAIL_ROWS, visible)));
}

async function fetchWindow(start, count, { merge = true } = {}) {
  const id = state.selectedId;
  if (!id) return null;
  const data = await api(
    `/api/session/${id}/screen?start_row=${start}&rows=${count}&cols=200`,
  );
  if (state.selectedId !== id) return null;
  if (merge) mergeWindow(data);
  return data;
}

async function openScreen() {
  if (!state.selectedId) return;
  const session = currentSession();
  if (!hasTerminal(session)) {
    renderTerminal();
    return;
  }

  state.rows.clear();
  state.window = null;
  state.pinned = true;
  renderTerminal();

  const head = await fetchWindow(0, 1, { merge: false });
  if (!head) return;
  const total = head.total_rows;
  if (total > 1) {
    const count = tailCount(total);
    const start = Math.max(0, total - count);
    await fetchWindow(start, Math.min(count, total - start));
  }
  renderTerminal();
  scrollToCursor();
  state.pinned = true;
  // A fresh shell can paint its prompt after the first read; settle once.
  const opened = state.selectedId;
  setTimeout(() => {
    if (state.selectedId !== opened) return;
    state.pinned = true;
    scrollToCursor();
    renderTerminal();
  }, 600);
}

async function refreshScreen() {
  if (state.view !== "screen" || !state.selectedId) return;
  const session = currentSession();
  if (!hasTerminal(session)) return;

  if (!state.window) {
    await openScreen();
    return;
  }

  const total = state.window.total_rows;
  const count = tailCount(total);
  const start = Math.max(0, total - count);
  if (count <= 0) return;
  await fetchWindow(start, count);
  renderTerminal();
  if (state.pinned) scrollToCursor();
}

async function ensureVisible() {
  const window_ = state.window;
  if (!window_ || state.loadingOlder) return;
  const screen = $("screen");
  const top = screen.scrollTop;
  const first = Math.max(0, Math.floor(top / ROW_PX));
  const last = Math.min(
    window_.total_rows,
    Math.ceil((top + (screen.clientHeight || 640)) / ROW_PX),
  );
  let missing = false;
  for (let row = first; row < last; row += 1) {
    if (!state.rows.has(row)) {
      missing = true;
      break;
    }
  }
  if (!missing) return;
  state.loadingOlder = true;
  try {
    const start = Math.max(0, first - 2);
    const count = Math.min(128, last + 2 - start);
    await fetchWindow(start, count);
    renderTerminal();
  } finally {
    state.loadingOlder = false;
  }
}

async function loadTranscript({ before = null, identity = null, older = false } = {}) {
  const id = state.selectedId;
  if (!id) return;
  const body = {};
  if (before !== null) body.before = before;
  if (identity !== null) body.identity = identity;
  const page = await post(`/api/session/${id}/transcript`, body);
  if (state.selectedId !== id) return;

  if (!older || !state.transcript) {
    state.transcript = { entries: page.entries, start: page.start, identity: page.identity };
  } else {
    state.transcript = {
      entries: [...page.entries, ...state.transcript.entries],
      start: page.start,
      identity: page.identity,
    };
  }
  renderTranscript();
}

async function toggleView() {
  if (state.view === "screen") {
    state.view = "chat";
    $("screen").hidden = true;
    $("transcript-view").hidden = false;
    if (!state.transcript) await loadTranscript();
  } else {
    state.view = "screen";
    $("transcript-view").hidden = true;
    $("screen").hidden = false;
    renderTerminal();
    if (state.pinned) scrollToCursor();
  }
  renderActions();
}

function refreshList() {
  if (state.listPromise) return state.listPromise;
  state.listPromise = refreshListOnce().finally(() => {
    state.listPromise = null;
  });
  return state.listPromise;
}

async function refreshListOnce() {
  state.listBusy = true;
  try {
    const data = await api("/api/state");
    state.sessions = data.sessions || [];
    state.projects = data.projects || [];
    state.agents = data.agents || [];
    state.lastOkAt = Date.now();
    hideBanner();
    $("subtitle").textContent = `${state.sessions.length} session${
      state.sessions.length === 1 ? "" : "s"
    }`;
    renderList();
    if (!$("new-sheet").hidden && !state.newBusy) syncNewForm();
    if (state.selectedId) {
      renderSessionHeader();
      renderMeta();
      renderActions();
      renderComposerState();
    }
  } catch (error) {
    if (Date.now() - state.lastOkAt > STALE_AFTER_MS) showBanner(error.message);
  } finally {
    state.listBusy = false;
  }
}

async function mutate(action, message) {
  await post(`/api/session/${state.selectedId}/${action}`, {});
  toast(message);
  await refreshList();
}

function syncPolling() {
  clearInterval(state.listTimer);
  clearInterval(state.screenTimer);
  state.listTimer = null;
  state.screenTimer = null;
  if (document.hidden) return;

  state.listTimer = setInterval(refreshList, LIST_POLL_MS);
  if (state.selectedId) {
    state.screenTimer = setInterval(() => {
    refreshScreen().catch((error) => showBanner(error.message));
  }, SCREEN_POLL_MS);
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
  state.window = null;
  state.rows.clear();
  state.transcript = null;
  state.view = "screen";
  state.pinned = true;
  $("transcript-view").hidden = true;
  $("screen").hidden = false;
  if (push && location.hash !== `#/s/${id}`) location.hash = `#/s/${id}`;
  $("list-view").hidden = true;
  $("session-view").hidden = false;
  $("back").hidden = false;
  $("new-session").hidden = true;
  renderSession();
  syncPolling();
  await refreshList();
  if (hasTerminal(currentSession())) {
    await openScreen().catch((error) => showBanner(error.message));
  }
  renderSession();
}

function back({ push = true } = {}) {
  state.selectedId = null;
  state.window = null;
  state.rows.clear();
  state.transcript = null;
  if (push && location.hash) location.hash = "";
  $("session-view").hidden = true;
  $("list-view").hidden = false;
  $("back").hidden = true;
  $("new-session").hidden = false;
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
    setTimeout(() => refreshScreen().catch((error) => showBanner(error.message)), 250);
  } catch (error) {
    showBanner(error.message);
  }
}

async function sendKey(key) {
  if (!state.selectedId) return;
  try {
    await post(`/api/session/${state.selectedId}/key`, { key });
    setTimeout(() => refreshScreen().catch((error) => showBanner(error.message)), 200);
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

const NEW_PROJECT_KEY = "ovrcr.new.project";
const NEW_WORKSPACE_KEY = "ovrcr.new.workspace";
const NEW_AGENT_KEY = "ovrcr.new.agent";

function remembered(key) {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
}

function remember(key, value) {
  try {
    localStorage.setItem(key, value);
  } catch {
    // Storage can be unavailable in private mode; the choice is only a default.
  }
}

function projectRecord(name) {
  return state.projects.find((item) => item.name === name) || null;
}

function workspaceRecord(projectName, id) {
  return projectRecord(projectName)?.workspaces?.find((item) => item.id === id) || null;
}

function newOption(value, label, { disabled = false } = {}) {
  const node = document.createElement("option");
  node.value = value;
  node.textContent = label;
  node.disabled = disabled;
  return node;
}

function chooseProject(preferred) {
  const names = state.projects.map((item) => item.name);
  if (preferred && names.includes(preferred)) return preferred;
  const stored = remembered(NEW_PROJECT_KEY);
  if (stored && names.includes(stored)) return stored;
  return names[0] || "";
}

function chooseWorkspace(projectName, preferred) {
  const project = projectRecord(projectName);
  const rows = (project?.workspaces || []).filter((item) => !item.setup_pending);
  if (preferred && rows.some((item) => item.id === preferred)) return preferred;
  const stored = remembered(`${NEW_WORKSPACE_KEY}.${projectName}`);
  if (stored && rows.some((item) => item.id === stored)) return stored;
  return rows[0]?.id || "";
}

function chooseAgent(preferred) {
  const names = state.agents.map((item) => item.name);
  if (preferred && names.includes(preferred)) return preferred;
  const stored = remembered(NEW_AGENT_KEY);
  if (stored && names.includes(stored)) return stored;
  for (const name of ["omp", "claude", "codex", "pi", "shell"]) {
    if (names.includes(name)) return name;
  }
  return names[0] || "";
}

function fillWorkspaces(projectName, preferred) {
  const select = $("new-workspace");
  const note = $("new-workspace-note");
  const rows = projectRecord(projectName)?.workspaces || [];
  select.replaceChildren();
  if (rows.length === 0) {
    select.append(newOption("", "No workspaces", { disabled: true }));
    note.textContent = "Create a workspace in OVRCR first.";
    note.hidden = false;
    return;
  }
  for (const workspace of rows) {
    const label = workspace.branch || workspace.id;
    const suffix = workspace.root ? " (root)" : workspace.setup_pending ? " · setting up…" : "";
    select.append(newOption(workspace.id, `${label}${suffix}`, { disabled: workspace.setup_pending }));
  }
  select.value = chooseWorkspace(projectName, preferred);
  const selected = workspaceRecord(projectName, select.value);
  note.textContent = selected?.setup_pending ? "This workspace is still setting up." : "";
  note.hidden = !selected?.setup_pending;
}

function syncAgentNote() {
  const agent = state.agents.find((item) => item.name === $("new-agent").value);
  const note = $("new-agent-note");
  if (!agent) {
    note.textContent = "";
  } else if (agent.name === "shell") {
    note.textContent = "Starts an empty shell; type commands after it opens.";
  } else if (agent.takes_task) {
    note.textContent = "The initial prompt is passed to the agent at launch.";
  } else {
    note.textContent = "This agent starts without the prompt; type it after it opens.";
  }
}

function syncNewForm() {
  const project = $("new-project");
  const currentProject = project.value;
  project.replaceChildren();
  for (const item of state.projects) project.append(newOption(item.name, item.name));
  project.value = chooseProject(currentProject);

  fillWorkspaces(project.value, $("new-workspace").value);

  const agent = $("new-agent");
  const currentAgent = agent.value;
  agent.replaceChildren();
  for (const item of state.agents) agent.append(newOption(item.name, item.label || item.name));
  agent.value = chooseAgent(currentAgent);
  syncAgentNote();
  $("new-create").disabled = !state.projects.length || !state.agents.length;
}

async function openNew() {
  if (!state.projects.length || !state.agents.length) {
    try {
      await refreshList();
    } catch (error) {
      showBanner(error.message);
      return;
    }
  }
  if (!state.projects.length) {
    toast("No projects registered. Add one with ovrcr project add.");
    return;
  }
  $("new-error").hidden = true;
  $("new-prompt").value = "";
  $("new-name").value = "";
  syncNewForm();
  $("new-sheet").hidden = false;
  $("new-close").focus();
}

function closeNew() {
  $("new-sheet").hidden = true;
  $("new-error").hidden = true;
  state.newBusy = false;
  $("new-create").disabled = false;
  $("new-session").focus();
}

async function createNew(event) {
  event.preventDefault();
  if (state.newBusy) return;
  const project = $("new-project").value;
  const workspace = $("new-workspace").value;
  const agent = $("new-agent").value;
  if (!project || !workspace || !agent) return;

  state.newBusy = true;
  $("new-create").disabled = true;
  $("new-error").hidden = true;
  try {
    const data = await post("/api/session", {
      project,
      workspace,
      agent,
      prompt: $("new-prompt").value,
      name: $("new-name").value.trim(),
    });
    remember(NEW_PROJECT_KEY, project);
    remember(`${NEW_WORKSPACE_KEY}.${project}`, workspace);
    remember(NEW_AGENT_KEY, agent);
    closeNew();
    await refreshList();
    toast("session started");
    if (data?.session?.id) await select(data.session.id);
  } catch (error) {
    $("new-error").textContent = error.message;
    $("new-error").hidden = false;
  } finally {
    state.newBusy = false;
    $("new-create").disabled = false;
  }
}

function wire() {
  $("back").addEventListener("click", () => back());
  $("new-session").addEventListener("click", () => {
    openNew().catch((error) => showBanner(error.message));
  });
  $("new-close").addEventListener("click", closeNew);
  $("new-cancel").addEventListener("click", closeNew);
  $("new-form").addEventListener("submit", createNew);
  $("new-project").addEventListener("change", () => {
    fillWorkspaces($("new-project").value, "");
  });
  $("new-agent").addEventListener("change", syncAgentNote);
  $("new-sheet").addEventListener("click", (event) => {
    if (event.target === $("new-sheet")) closeNew();
  });
  document.addEventListener("keydown", (event) => {
    if (event.key === "Escape" && !$("new-sheet").hidden) {
      event.preventDefault();
      closeNew();
    }
  });
  $("banner-dismiss").addEventListener("click", hideBanner);
  $("composer").addEventListener("submit", (event) => {
    event.preventDefault();
    sendText(true);
  });
  $("type-only").addEventListener("click", () => sendText(false));
  $("load-older").addEventListener("click", () => {
    if (!state.transcript) return;
    loadTranscript({
      before: state.transcript.start,
      identity: state.transcript.identity,
      older: true,
    }).catch((error) => showBanner(error.message));
  });
  $("text").addEventListener("keydown", (event) => {
    if ((event.metaKey || event.ctrlKey) && event.key === "Enter") {
      event.preventDefault();
      sendText(true);
    }
  });
  $("screen").addEventListener("scroll", () => {
    const screen = $("screen");
    if (
      state.programmaticTop !== null &&
      Math.abs(screen.scrollTop - state.programmaticTop) < 2
    ) {
      state.programmaticTop = null;
      return;
    }
    state.pinned = atCursor();
    if (state.scrollFrame) return;
    state.scrollFrame = requestAnimationFrame(() => {
      state.scrollFrame = null;
      renderTerminal();
    });
    ensureVisible().catch((error) => showBanner(error.message));
  });
  document.addEventListener("visibilitychange", () => {
    syncPolling();
    if (!document.hidden) {
      refreshList();
      if (state.selectedId) refreshScreen();
    }
  });
  window.addEventListener("hashchange", route);
  window.addEventListener("resize", () => {
    if (state.view === "screen" && state.selectedId) {
      refreshScreen().catch((error) => showBanner(error.message));
    }
  });
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
