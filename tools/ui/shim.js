// Stands in for Tauri's internals when the frontend runs in a browser
// against crates/core/examples/ui_bridge.rs: invoke() becomes a POST to the
// bridge, events come from long polling. Calls, events and page errors are
// kept in window.__calls, __events and __errors for the tests to read.
(function () {
  const callbacks = new Map();
  const listeners = new Map(); // event -> [{eventId, handlerId}]
  let nextId = 1;
  window.__calls = [];
  window.__errors = [];
  window.addEventListener('error', (e) => window.__errors.push(String(e.message)));
  window.addEventListener('unhandledrejection', (e) => window.__errors.push('unhandled: ' + String(e.reason)));
  window.__TAURI_EVENT_PLUGIN_INTERNALS__ = {
    unregisterListener(event, eventId) {
      const l = listeners.get(event) || [];
      listeners.set(event, l.filter((x) => x.eventId !== eventId));
    },
  };
  window.__TAURI_INTERNALS__ = {
    metadata: { currentWindow: { label: 'main' }, currentWebview: { windowLabel: 'main', label: 'main' } },
    transformCallback(cb, once) {
      const id = nextId++;
      callbacks.set(id, (x) => { if (once) callbacks.delete(id); return cb && cb(x); });
      return id;
    },
    unregisterCallback(id) { callbacks.delete(id); },
    convertFileSrc(p) { return p; },
    async invoke(cmd, args = {}) {
      if (cmd === 'plugin:event|listen') {
        const eventId = nextId++;
        const l = listeners.get(args.event) || [];
        l.push({ eventId, handlerId: args.handler });
        listeners.set(args.event, l);
        return eventId;
      }
      if (cmd === 'plugin:event|unlisten') return null;
      if (cmd.startsWith('plugin:')) { window.__calls.push({ cmd, args }); return null; }
      const t0 = performance.now();
      const r = await fetch('/invoke/' + cmd, { method: 'POST', body: JSON.stringify(args) });
      const body = await r.json();
      window.__calls.push({ cmd, args, ms: Math.round(performance.now() - t0), ok: r.ok });
      if (!r.ok) throw body;
      return body;
    },
  };
  let since = 0;
  window.__events = [];
  async function poll() {
    for (;;) {
      try {
        const evs = await (await fetch('/events?since=' + since)).json();
        for (const e of evs) {
          since = e.n + 1;
          window.__events.push({ event: e.event, payload: e.payload, t: Date.now() });
          for (const l of listeners.get(e.event) || []) {
            const cb = callbacks.get(l.handlerId);
            if (cb) cb({ event: e.event, id: l.eventId, payload: e.payload });
          }
        }
      } catch (e) {
        await new Promise((r) => setTimeout(r, 500));
      }
    }
  }
  poll();
})();
