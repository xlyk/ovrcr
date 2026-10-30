// Executed via mcp__cua_repl.js, after its current API documentation was loaded.
// File writes preserve evidence; all UI observation goes through the bound app.
var fs = await import('node:fs/promises');
var zlib = await import('node:zlib');
var crypto = await import('node:crypto');
var ev = '/Users/xlyk/.codex/worktrees/operator-layout-design/ovrcr/research/operator-activity-2026-10-30/native-01';
var capture = async function(name) {
  var started = Date.now();
  var result = await app.getAXStateAndScreenshot({disableDiffing:true, emit:false});
  var ended = Date.now();
  var bytes = result.screenshot || result.screenshots[0].bytes;
  var metadata = {
    call: 'app.getAXStateAndScreenshot({disableDiffing:true,emit:false})',
    started_unix_ms: started,
    ended_unix_ms: ended,
    source_revision: 'ace3077b0c6743648b96b6986c3cac9ce50e7a07',
    screenshot_bytes: bytes.length,
    screenshot_sha256: crypto.createHash('sha256').update(bytes).digest('hex'),
    ax_sha256: crypto.createHash('sha256').update(result.state).digest('hex'),
    activity_lines: result.state.split('\n').filter(s => s.includes('Responding') || s.includes('Waiting for') || s.includes('Managing'))
  };
  await fs.writeFile(ev + '/' + name + '.jpg', bytes);
  await fs.writeFile(ev + '/' + name + '.ax.txt.gz', zlib.gzipSync(result.state, {mtime:0}));
  await fs.writeFile(ev + '/' + name + '.json', JSON.stringify(metadata, null, 2) + '\n');
  nodeRepl.write(metadata);
  await nodeRepl.emitImage(bytes);
};
// First capture tool invocation: await capture('01-silent-hidden-a');
// Immediately following tool invocation: await capture('02-silent-hidden-b');
// No input, resize, provider release or other state mutation between those calls.
