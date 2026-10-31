import { test } from 'node:test';
import assert from 'node:assert/strict';
import { ready, run, start, shutdownAll } from './process.mjs';
test('nonzero child status fails the suite', async () => { await assert.rejects(run(process.execPath, ['-e', 'process.exit(7)']), /failed \(7\)/); });
test('missing executable preserves spawn error', async () => { await assert.rejects(run('capntproto-nonexistent-example-test-command', []), { code: 'ENOENT' }); });
test('explicit readiness and backend cleanup', async () => {
  const backend = await ready(process.execPath, ['-e', 'console.log(JSON.stringify({address:"127.0.0.1:12345"}));setInterval(()=>{},1000)']);
  assert.equal(backend.address, '127.0.0.1:12345'); await backend.stop();
});
test('early backend failure rejects readiness', async () => { await assert.rejects(ready(process.execPath, ['-e', 'process.exit(5)']), /exited before readiness/); });
test('global cleanup reaps children', async () => { const child = start(process.execPath, ['-e', 'setInterval(()=>{},1000)']); await shutdownAll(); assert.notEqual((await child.exited).code, 0); });
