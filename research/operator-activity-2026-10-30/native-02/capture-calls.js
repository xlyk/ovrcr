// Executed via mcp__cua_repl.js, after its current API documentation was loaded.
// File writes preserve evidence; all UI observation goes through the bound app.
var fs = await import('node:fs/promises');
var zlib = await import('node:zlib');
var crypto = await import('node:crypto');
var ev2 = '/Users/xlyk/.codex/worktrees/operator-layout-design/ovrcr/research/operator-activity-2026-10-30/native-02';
var capture2 = async function(name) {
  var started = Date.now();
  var result = await app2.getAXStateAndScreenshot({disableDiffing:true, emit:false});
  var ended = Date.now();
  var bytes = result.screenshot || result.screenshots[0].bytes;
  var metadata = {
    call: 'app2.getAXStateAndScreenshot({disableDiffing:true,emit:false})',
    started_unix_ms: started,
    ended_unix_ms: ended,
    source_revision: 'ace3077b0c6743648b96b6986c3cac9ce50e7a07',
    screenshot_bytes: bytes.length,
    screenshot_sha256: crypto.createHash('sha256').update(bytes).digest('hex'),
    ax_sha256: crypto.createHash('sha256').update(result.state).digest('hex'),
    activity_lines: result.state.split('\n').filter(s => s.includes('Responding') || s.includes('Waiting for') || s.includes('Managing') || s.includes('▸ 1') || s.includes('· complete'))
  };
  await fs.writeFile(ev2 + '/' + name + '.jpg', bytes);
  await fs.writeFile(ev2 + '/' + name + '.ax.txt.gz', zlib.gzipSync(result.state, {mtime:0}));
  await fs.writeFile(ev2 + '/' + name + '.json', JSON.stringify(metadata, null, 2) + '\n');
  nodeRepl.write(metadata);
  await nodeRepl.emitImage(bytes);
};
// First capture tool invocation: await capture2('01-pending-collapsed-a');
// Immediately following tool invocation: await capture2('02-pending-collapsed-b');
// No input, resize, provider release or other state mutation between those calls.

// After pending-after authority receipt and one release of each held gate:
// await capture2('03-completed-collapsed');
// Fresh full AX then identified 73 close button; await app2.click(73).
// No closed-app query followed that click.
