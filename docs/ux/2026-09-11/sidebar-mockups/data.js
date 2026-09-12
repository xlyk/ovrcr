// Shared fixture + cell-grid helpers. Every row is exactly W cells wide so
// each mockup stays renderable by the ratatui sidebar (DEFAULT_SIDEBAR_WIDTH = 40).
const W = 40;

const DATA = [
  {name:'consigint', workspaces:[
    {name:'auth-handoff', sessions:[
      {name:'local', label:'terminal', phase:'running', activity:'idle', elapsed:'2h04m', ctx:null, pid:48102},
      {name:'review auth handoff', label:'grok / grok-4.6', phase:'running', activity:'busy', elapsed:'12m', ctx:41, pid:48213},
    ]},
    {name:'worktree-lifecycle', sessions:[
      {name:'local', label:'terminal', phase:'running', activity:'idle', elapsed:'3h11m', ctx:null, pid:47990},
      {name:'implement lifecycle cleanup', label:'claude / sonnet-4', phase:'running', activity:'waiting', elapsed:'48m', ctx:73, pid:48077},
      {name:'review websocket shutdown', label:'codex / gpt-5.4', phase:'running', activity:'busy', elapsed:'6m', ctx:22, pid:48301},
      {name:'plan snapshot restore', label:'pi / grok-4.6', phase:'exited', activity:'idle', elapsed:'1h12m', ctx:58, pid:null},
    ]},
  ]},
  {name:'spacelift-agent', workspaces:[
    {name:'pipeline-progress-v2', sessions:[
      {name:'local', label:'terminal', phase:'running', activity:'idle', elapsed:'55m', ctx:null, pid:48150},
      {name:'build pipeline progress', label:'claude / opus-4', phase:'running', activity:'error', elapsed:'23m', ctx:66, pid:48222},
    ]},
    {name:'scope-quality', sessions:[
      {name:'local', label:'terminal', phase:'running', activity:'idle', elapsed:'19m', ctx:null, pid:48290},
      {name:'review scope quality', label:'codex / gpt-5.4', phase:'running', activity:'unknown', elapsed:'3m', ctx:null, pid:48333},
    ]},
  ]},
];
{ let id=0; DATA.forEach(p=>p.workspaces.forEach(w=>w.sessions.forEach(s=>{s.id=id++; s.project=p.name; s.workspace=w.name;}))); }
const ALL = DATA.flatMap(p=>p.workspaces.flatMap(w=>w.sessions));

// label_color() in render.rs
const PROVIDER = {claude:{tag:'cl',cls:'peach'}, codex:{tag:'cx',cls:'green'}, grok:{tag:'gk',cls:'blue'}, pi:{tag:'pi',cls:'mauve'}, terminal:{tag:'$',cls:'subtext'}};
const provider = s => s.label.split('/')[0].trim().toLowerCase();
const model = s => (s.label.split('/')[1]||'').trim();
const pinfo = s => PROVIDER[provider(s)] || {tag:'??',cls:'text'};

const SPIN = ['⠋','⠙','⠹','⠸','⠼','⠴','⠦','⠧','⠇','⠏'];
// [glyph, colour class, animated?]
function status(s){
  if(s.phase==='exited') return ['·','muted',false];
  if(s.phase==='paused') return ['‖','subtext',false];
  switch(s.activity){
    case 'busy':    return [SPIN[0],'green',true];
    case 'waiting': return ['?','yellow',false];
    case 'error':   return ['!','red',false];
    case 'unknown': return ['-','muted',false];
    default:        return [' ','muted',false];
  }
}
const ACTIVITY = {busy:'agent busy', waiting:'agent waiting input', error:'agent error', unknown:'agent unknown', idle:'agent idle'};
let frame=0;
setInterval(()=>{ frame=(frame+1)%SPIN.length; document.querySelectorAll('.spin').forEach(e=>e.textContent=SPIN[frame]); },100);

const len  = s => [...s].length;
const esc  = s => s.replace(/&/g,'&amp;').replace(/</g,'&lt;');
const trunc = (s,n) => len(s)>n ? [...s].slice(0,Math.max(0,n-1)).join('')+'…' : s;

// seg = [text, cls, attrs?]. Pads between left and right so the row is exactly W cells.
function line(left, right=[], rowCls='', attrs=''){
  const l = left.map(x=>x[0]).join(''), r = right.map(x=>x[0]).join('');
  let gap = W - len(l) - len(r);
  if(gap<0){ const last=left[left.length-1]; left[left.length-1]=[trunc(last[0], Math.max(1,len(last[0])+gap-1)), last[1], last[2]]; gap=1; }
  const html = [...left, [' '.repeat(gap),''], ...right].map(([t,c,a])=>`<span class="${c||''}" ${a||''}>${esc(t)}</span>`).join('');
  return `<div class="row ${rowCls}" ${attrs}>${html}</div>`;
}
const rule = () => line([['─'.repeat(W),'surface']]);
const quietHeader = (right=[]) => line([[' OVRCR','text b'],['  agent runtime','muted']], right, 'hdr-line');

function counts(sessions){ const c={busy:0,waiting:0,error:0}; sessions.forEach(s=>{ if(s.phase==='running' && c[s.activity]!==undefined) c[s.activity]++; }); return c; }
function rollup(c){
  const segs=[]; if(c.busy) segs.push(['● '+c.busy,'green']); if(c.waiting) segs.push(['? '+c.waiting,'yellow']); if(c.error) segs.push(['! '+c.error,'red']);
  return segs.flatMap((s,i)=> i? [[' ',''],s] : [s]);
}

const state = { selected: 0, collapsed: new Set() };
const isCollapsed = k => state.collapsed.has(k);
const selectedSession = () => ALL.find(s=>s.id===state.selected);
function bind(root, render){
  root.addEventListener('click', e=>{
    const sid=e.target.closest('[data-sid]'), fold=e.target.closest('[data-fold]'), ws=e.target.closest('[data-ws]');
    if(sid) state.selected=+sid.dataset.sid;
    else if(fold){ const k=fold.dataset.fold; state.collapsed.has(k)?state.collapsed.delete(k):state.collapsed.add(k); }
    else if(ws){ const [pn,wn]=ws.dataset.ws.split('/'); const w=DATA.find(p=>p.name===pn).workspaces.find(w=>w.name===wn); if(w.sessions[0]) state.selected=w.sessions[0].id; }
    else return;
    render();
  });
}
