import { instantiate } from '../target/browser/frontend.js';
import { transport } from './transport.js';
import { monotonicClock } from './clock.js';
const core = new URL('../target/browser/', import.meta.url);
export const frontend = await instantiate(name => WebAssembly.compileStreaming(fetch(new URL(name, core))), {
  'capntproto:browser/transport': transport, 'wasi:clocks/monotonic-clock': monotonicClock,
});
const form = document.querySelector('form'); form.dataset.ready = 'true'; form.querySelector('button').disabled = false;
form.addEventListener('submit', async event => {
  event.preventDefault(); const output = document.querySelector('output'); delete output.dataset.state; output.textContent = 'Connecting…';
  try {
    const data = new FormData(form); const hex = String(data.get('pin')).replaceAll(':', '').trim();
    if (hex && !/^[0-9a-f]{64}$/i.test(hex)) throw new Error('Enter 64 hexadecimal SHA-256 digits');
    const pin = hex ? Uint8Array.from(hex.match(/../g), h => parseInt(h, 16)) : new Uint8Array();
    output.textContent = await frontend.greet(String(data.get('url')), pin, String(data.get('prefix')), String(data.get('value'))); output.dataset.state = 'success';
  } catch (error) { output.textContent = String(error); output.dataset.state = 'error'; }
});
