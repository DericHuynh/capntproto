import { spawn } from 'node:child_process';
import { createInterface } from 'node:readline';
const cleanups = new Set();
export function registerCleanup(cleanup) { cleanups.add(cleanup); return () => cleanups.delete(cleanup); }
export async function shutdownAll() { await Promise.allSettled([...cleanups].map(cleanup => cleanup())); }
export function start(command, args, { cwd, env = process.env, pipe = false } = {}) {
  const child = spawn(command, args, { cwd, env, detached: process.platform !== 'win32', stdio: pipe ? ['ignore', 'pipe', 'pipe'] : 'inherit' });
  let finished = false;
  const exited = new Promise((resolve, reject) => {
    child.once('error', error => { finished = true; reject(error); });
    child.once('exit', (code, signal) => { finished = true; resolve({ code, signal }); });
  });
  const stop = async () => {
    if (finished) return;
    const kill = signal => { try { if (process.platform !== 'win32') process.kill(-child.pid, signal); else child.kill(signal); } catch (error) { if (error.code !== 'ESRCH') throw error; } };
    kill('SIGTERM'); let timer;
    await Promise.race([exited, new Promise(resolve => { timer = setTimeout(() => { kill('SIGKILL'); resolve(); }, 2000); })]);
    clearTimeout(timer); await exited;
  };
  const unregister = registerCleanup(stop); exited.then(unregister, unregister); return { child, exited, stop };
}
export async function run(command, args, options = {}) {
  console.log('+', command, ...args); const running = start(command, args, options);
  const timer = setTimeout(() => running.stop(), options.timeout ?? 600_000);
  try { const { code, signal } = await running.exited; if (code !== 0) throw new Error(`${command} failed (${signal ?? code})`); }
  finally { clearTimeout(timer); await running.stop(); }
}
export async function ready(command, args, options = {}) {
  const running = start(command, args, { ...options, pipe: true }); running.child.stderr.on('data', chunk => process.stderr.write(chunk));
  const lines = createInterface({ input: running.child.stdout }); let timer;
  try {
    const address = await Promise.race([
      new Promise(resolve => { lines.on('line', line => { console.log(line); try { const value = JSON.parse(line); if (value.address) resolve(value.address); } catch {} }); }),
      running.exited.then(({ code, signal }) => { throw new Error(`backend exited before readiness (${signal ?? code})`); }),
      new Promise((_, reject) => { timer = setTimeout(() => reject(new Error('backend readiness deadline exceeded')), 30_000); }),
    ]);
    return { address, stop: async () => { lines.close(); await running.stop(); } };
  } catch (error) { lines.close(); await running.stop(); throw error; }
  finally { clearTimeout(timer); }
}
