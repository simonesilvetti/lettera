import { spawn } from 'node:child_process';
import http from 'node:http';
import fs from 'node:fs';

// Headless Chromium driven over the DevTools protocol, controlled over HTTP
// on :8978 so each test step can be a separate command (see t.py).
const S = process.env.UI_TMP ?? '/tmp/lettera-ui';
fs.mkdirSync(S, { recursive: true });
const chrome = spawn('chromium', ['--headless=new', '--remote-debugging-port=9333', `--user-data-dir=${S}/chrome`, '--window-size=1400,900', '--no-first-run', '--disable-gpu', 'about:blank'], { stdio: 'ignore' });
process.on('exit', () => chrome.kill());
await new Promise((r) => setTimeout(r, 1500));
let targets;
for (let i = 0; i < 20; i++) {
  try { targets = await (await fetch('http://127.0.0.1:9333/json')).json(); break; } catch { await new Promise((r) => setTimeout(r, 500)); }
}
const page = targets.find((t) => t.type === 'page');
const ws = new WebSocket(page.webSocketDebuggerUrl);
await new Promise((r) => ws.addEventListener('open', r));
let id = 0;
const pending = new Map();
const consoleLog = [];
ws.addEventListener('message', (m) => {
  const d = JSON.parse(m.data);
  if (d.id && pending.has(d.id)) { pending.get(d.id)(d); pending.delete(d.id); }
  if (d.method === 'Runtime.consoleAPICalled') consoleLog.push(d.params.type + ': ' + d.params.args.map((a) => a.value ?? a.description).join(' '));
  if (d.method === 'Runtime.exceptionThrown') consoleLog.push('EXC: ' + JSON.stringify(d.params.exceptionDetails.exception?.description ?? d.params.exceptionDetails.text));
});
const cdp = (method, params = {}) => new Promise((r) => { const i = ++id; pending.set(i, r); ws.send(JSON.stringify({ id: i, method, params })); });
await cdp('Runtime.enable');
await cdp('Page.enable');
await cdp('Emulation.setDeviceMetricsOverride', { width: 1400, height: 900, deviceScaleFactor: 1, mobile: false });

const KEYS = { Escape: 27, Enter: 13, Tab: 9, Backspace: 8, ArrowDown: 40, ArrowUp: 38, ArrowLeft: 37, ArrowRight: 39, Delete: 46, Home: 36, End: 35, PageDown: 34, PageUp: 33, ' ': 32 };
async function key(spec) {
  // spec like "j", "ctrl+d", "shift+Tab", "Escape"
  const parts = spec.split('+');
  let k = parts.pop();
  if (k === '' ) k = '+';
  const mods = parts.reduce((m, p) => m | ({ alt: 1, ctrl: 2, meta: 4, shift: 8 }[p] || 0), 0);
  const isChar = k.length === 1;
  const code = isChar ? (/[a-z]/i.test(k) ? 'Key' + k.toUpperCase() : /\d/.test(k) ? 'Digit' + k : '') : k;
  const vk = KEYS[k] ?? (isChar ? k.toUpperCase().charCodeAt(0) : 0);
  const text = isChar && !(mods & 2) && !(mods & 4) ? k : k === 'Enter' && !(mods & 2) ? '\r' : undefined;
  await cdp('Input.dispatchKeyEvent', { type: text ? 'keyDown' : 'rawKeyDown', key: k, code, windowsVirtualKeyCode: vk, modifiers: mods, text });
  await cdp('Input.dispatchKeyEvent', { type: 'keyUp', key: k, code, windowsVirtualKeyCode: vk, modifiers: mods });
}
async function evaluate(expr) {
  const r = await cdp('Runtime.evaluate', { expression: expr, awaitPromise: true, returnByValue: true });
  if (r.result.exceptionDetails) return { error: r.result.exceptionDetails.exception?.description || r.result.exceptionDetails.text };
  return r.result.result.value;
}
http.createServer(async (req, res) => {
  let body = '';
  for await (const c of req) body += c;
  let a = {};
  try { a = body ? JSON.parse(body) : {}; } catch (e) { res.end(JSON.stringify({ error: "bad json" })); return; }
  let out;
  try {
    switch (req.url) {
      case '/goto': await cdp('Page.navigate', { url: a.url }); await new Promise((r) => setTimeout(r, a.wait ?? 1500)); out = 'ok'; break;
      case '/eval': out = await evaluate(a.expr); break;
      case '/keys': for (const k of a.keys) { await key(k); await new Promise((r) => setTimeout(r, a.gap ?? 60)); } await new Promise((r) => setTimeout(r, a.wait ?? 300)); out = 'ok'; break;
      case '/type': await cdp('Input.insertText', { text: a.text }); await new Promise((r) => setTimeout(r, a.wait ?? 200)); out = 'ok'; break;
      case '/click': {
        const box = await evaluate(`(() => { const e = ${a.js ?? `document.querySelector(${JSON.stringify(a.sel)})`}; if (!e) return null; e.scrollIntoView({block:'center'}); const r = e.getBoundingClientRect(); return {x: r.x + r.width/2, y: r.y + r.height/2}; })()`);
        if (!box) { out = 'not found'; break; }
        for (const type of ['mousePressed', 'mouseReleased']) await cdp('Input.dispatchMouseEvent', { type, x: box.x, y: box.y, button: 'left', clickCount: 1 });
        await new Promise((r) => setTimeout(r, a.wait ?? 400)); out = 'ok'; break;
      }
      case '/shot': { const r = await cdp('Page.captureScreenshot', { format: 'png' }); fs.writeFileSync(a.path, Buffer.from(r.result.data, 'base64')); out = a.path; break; }
      case '/cdp': out = await cdp(a.method, a.params || {}); break;
      case '/console': out = consoleLog.splice(0); break;
      default: out = 'unknown';
    }
  } catch (e) { out = { error: String(e) }; }
  res.end(JSON.stringify(out));
}).listen(8978, () => console.log('driver ready'));
