#!/usr/bin/env python3
"""t.py eval JS | keys k1 k2 ... | type TEXT | click SEL | clickjs JS | shot NAME | goto URL | console | state"""
import json, sys, urllib.request, os
S = os.environ.get('UI_TMP', '/tmp/lettera-ui')
def post(ep, data):
    r = urllib.request.urlopen(urllib.request.Request('http://127.0.0.1:8978/' + ep, data=json.dumps(data).encode()), timeout=120)
    raw = r.read()
    return json.loads(raw) if raw.strip() else None
cmd, args = sys.argv[1], sys.argv[2:]
try:
    import socket; socket.create_connection(("127.0.0.1", 8978), 2).close()
except OSError:
    sys.exit("driver down")
wait = int(os.environ.get('W', '0')) or None
if cmd == 'eval': out = post('eval', {'expr': args[0]})
elif cmd == 'keys': out = post('keys', {'keys': args, **({'wait': wait} if wait else {})})
elif cmd == 'type': out = post('type', {'text': args[0]})
elif cmd == 'click': out = post('click', {'sel': args[0], **({'wait': wait} if wait else {})})
elif cmd == 'clickjs': out = post('click', {'js': args[0], **({'wait': wait} if wait else {})})
elif cmd == 'shot': out = post('shot', {'path': f'{S}/{args[0]}.png'})
elif cmd == 'goto': out = post('goto', {'url': args[0], 'wait': 3000})
elif cmd == 'size': out = post('cdp', {'method': 'Emulation.setDeviceMetricsOverride', 'params': {'width': int(args[0]), 'height': int(args[1]), 'deviceScaleFactor': 1, 'mobile': False}})
elif cmd == 'console': out = post('console', {})
elif cmd == 'state':
    out = post('eval', {'expr': '''JSON.stringify({errors: window.__errors.splice(0), calls: window.__calls.splice(0).map(c=>c.cmd+":"+c.ms+(c.ok===false?" FAIL "+JSON.stringify(c.args).slice(0,80):"")),
      flash: document.querySelector('.flash, .status .msg, footer .flash')?.innerText, focus: document.activeElement?.tagName,
      header: document.querySelector('header')?.innerText.replace(/\\n/g,' | ')})'''})
else: out = 'unknown'
print(out if isinstance(out, str) else json.dumps(out, ensure_ascii=False, indent=1))
