// Executed with current mcp__cua_repl documentation and the already-running app.
var app = await cua.getApp('/Users/xlyk/.codex/worktrees/operator-layout-design/ovrcr/target/OVRCR GUI.app');
var fs = await import('node:fs/promises');
var zlib = await import('node:zlib');
var crypto = await import('node:crypto');
var ev = '/Users/xlyk/.codex/worktrees/operator-layout-design/ovrcr/research/operator-inline-input-2026-10-30/native-02';
var capture = async function(name) {
  var start = Date.now();
  var r = await app.getAXStateAndScreenshot({disableDiffing:true,emit:false});
  var b = r.screenshot || r.screenshots[0].bytes;
  var m = {
    call:'app.getAXStateAndScreenshot({disableDiffing:true,emit:false})',
    start_unix_ms:start, end_unix_ms:Date.now(),
    source_revision:'6e560817477ed9993c5806da958e39dd239b1511',
    jpg_sha256:crypto.createHash('sha256').update(b).digest('hex'),
    ax_sha256:crypto.createHash('sha256').update(r.state).digest('hex'),
    activity:r.state.split('\n').filter(s=>s.includes('Responding')||s.includes('Waiting for')||s.includes('Managing')||s.includes('> Retained'))
  };
  await fs.writeFile(ev+'/'+name+'.jpg',b);
  await fs.writeFile(ev+'/'+name+'.ax.txt.gz',zlib.gzipSync(r.state,{mtime:0}));
  await fs.writeFile(ev+'/'+name+'.json',JSON.stringify(m,null,2)+'\n');
  nodeRepl.write(m);
  await nodeRepl.emitImage(b);
};

// Consecutive capture-only calls, each in its own tool invocation:
// await capture('01-silent-collapsed-a');
// await capture('02-silent-collapsed-b');
// Existing model-start gate was then released once. Native resize 1100x932→1100x604.
// await capture('03-pending-collapsed-a');
// await capture('04-pending-collapsed-b');
// No input/resize/gate change/provider progress occurred within either pair.

// PageUp, capture05, capture06; PageDown returned the tail.
// Ctrl-g, b, o hid the sidebar and returned the editor; resize to723x406; capture07.
// PageUp/capture08; release Bash-result + final-model gates once; capture09.
// Two PageDown/capture10 returned the completed tail with the retained draft.

// Both full (11–13) and Quick (18–20) used this exact editing sequence:
// paste('alpha 界🙂 omega\nsecond café tail'), capture-before;
// Ctrl+A, Ctrl+F, Ctrl+D, Ctrl+B, Ctrl+F, Ctrl+H, Ctrl+E, Ctrl+W,
// Alt+B, Alt+F, Ctrl+K, Ctrl+A, Ctrl+F, Ctrl+U, capture-after.
// Result: 'alpha 界🙂 omega\nond café', cursor at second-line start.
// Home, Alt+F, Ctrl+F, Ctrl+D, Ctrl+F, Ctrl+H; capture-grapheme-delete.
// Result: 'alpha  omega\nond café', with 界 and 🙂 removed as whole graphemes.

// Real fixture-a and fixture-b conversations were created/submitted through UI.
// A nonempty draft/capture14; select B then A; Ctrl+C/capture15 left both Running.
// A separate Ctrl+C/capture16 interrupted A. Select B/capture17 remained Running.
// Ctrl-g/Ctrl-o opened Quick over B. Above edits then two separate Ctrl+C/capture21:
// first clear, second empty consumed no-op; B remained Running. Escape dismissed.
// b showed sidebar; resize1003x496; capture22 shows short B journal.

// Capture23 preserves a malformed quoted trap command in owned root shell #1.
// It exited that shell; this is harness failure, not application acceptance.
// Corrected proof selected the existing feature shell #4 using native sidebar click.
// paste("rtk proxy printf 'CUA_%s\\n' SIGINT_READY; rtk proxy sleep 30"), Return.
// Record actual owned sleep PIDs; capture24; Ctrl+C; paste and Return:
// "rtk proxy printf 'CUA_%s\\n' TERMINAL_SURVIVED"; capture25.
// Ctrl-g/o; paste('Review the next workspace change 界🙂'); capture26.
// Release B model gate once (and retire A provider gate); capture27 completes B.

// After final database/process receipt, finish holder once. Fresh full AX exposed
// 39 close button. await app.click(39) returned, followed by original waits0/0.
// No app query followed close. Kernel-only cleanup verified24 PIDs/19 groups absent.
