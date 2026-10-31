//! Shared local/CI entry point. Every failed check exits nonzero.
import assert from 'node:assert/strict';
import { mkdtemp, readFile, rm, writeFile, mkdir } from 'node:fs/promises';
import os from 'node:os'; import path from 'node:path'; import { fileURLToPath } from 'node:url';
import { componentNew, componentWit } from '@bytecodealliance/jco';
import { run, shutdownAll } from './process.mjs'; import { browserTests } from './browser.mjs';
for (const [signal, code] of [['SIGINT',130],['SIGTERM',143]]) process.once(signal, () => { shutdownAll().finally(() => process.exit(code)); });
const root = fileURLToPath(new URL('../..', import.meta.url)); const target = path.resolve(root, process.env.CARGO_TARGET_DIR || 'target');
const flags = new Set(process.argv.slice(2)); for (const flag of flags) assert(['--native-only','--browser-only','--no-build'].includes(flag), `unknown option ${flag}`);
assert(!(flags.has('--native-only') && flags.has('--browser-only')), 'choose at most one subset'); process.env.CARGO_TARGET_DIR = target;
const cargo = args => run('cargo', args, { cwd: root }); const manifest = name => `examples/${name}/Cargo.toml`;
await run(process.execPath, ['--test', path.join(root,'examples/integration/process.test.mjs')], { cwd: root });
if (!flags.has('--browser-only')) {
  await cargo(['test','--locked','--manifest-path',manifest('configurations')]);
  await cargo(['test','--locked','--manifest-path',manifest('configurations'),'--no-default-features','--test','matrix']);
  for (const features of ['monoio','webtransport']) await cargo(['check','--locked','--manifest-path',manifest('configurations'),'--no-default-features','--features',features,'--all-targets']);
  await cargo(['test','--locked','--manifest-path',manifest('embedded-tls'),'--features','host']);
  for (const features of ['','embassy-net']) await cargo(['check','--locked','--manifest-path',manifest('embedded-tls'),'--no-default-features','--features',features,'--target','thumbv7em-none-eabi','--lib']);
}
if (!flags.has('--native-only')) {
  if (!flags.has('--no-build')) { await cargo(['build','--locked','--manifest-path',manifest('configurations'),'--bins']); await cargo(['build','--locked','--manifest-path',manifest('wasip3'),'--target','wasm32-unknown-unknown','--lib']); }
  const output = path.join(root,'examples/wasip3/target'); await mkdir(output, { recursive: true });
  const component = await componentNew(await readFile(path.join(target,'wasm32-unknown-unknown/debug/capntproto_wasip3_example.wasm'))); const wit = await componentWit(component);
  assert(wit.includes('wasi:clocks/monotonic-clock@0.3.0')); assert(wit.includes('greet: async func'));
  await writeFile(path.join(output,'frontend.component.wasm'), component); await writeFile(path.join(output,'frontend.wit'), wit);
  await run(process.execPath, [path.join(root,'examples/integration/node_modules/@bytecodealliance/jco/dist/jco.js'),'transpile',path.join(output,'frontend.component.wasm'),'-o',path.join(output,'browser'),'--name','frontend','--instantiation','async','--no-nodejs-compat','--no-wasi-shim'], { cwd: root });
  const credentials = await mkdtemp(path.join(os.tmpdir(),'capntproto-examples-'));
  try {
    await run(path.join(target,`debug/example-credentials${process.platform === 'win32' ? '.exe' : ''}`), [credentials], { cwd: root }); let timer;
    try { await Promise.race([browserTests({ root, target, credentials }), new Promise((_, reject) => { timer = setTimeout(() => reject(new Error('browser integration deadline exceeded')),180_000); })]); }
    finally { clearTimeout(timer); await shutdownAll(); }
  } finally { await rm(credentials, { recursive: true, force: true }); }
}
console.log('PASS requested example integrations');
