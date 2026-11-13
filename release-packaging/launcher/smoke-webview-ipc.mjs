// Calls read-only IPC commands in a running webview through its remote
// debugging protocol and prints one JSON object. Test-only.
// WebView2: start the app with WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=<port>
//   and run: node smoke-webview-ipc.mjs <port> <command>...
// WebKitGTK: start the app with WEBKIT_INSPECTOR_HTTP_SERVER=127.0.0.1:<port>
//   and run: node smoke-webview-ipc.mjs webkit:<port> <command>...
import { randomBytes } from 'node:crypto';

const [target, ...commands] = process.argv.slice(2);
const webkit = target?.startsWith('webkit:') ?? false;
const port = webkit ? target.slice('webkit:'.length) : target;
if (!port || commands.length === 0) {
  console.error('usage: node smoke-webview-ipc.mjs <port> <command>...');
  process.exit(2);
}

// A version 7 UUID, as the bridge requires for correlation IDs.
function uuidV7() {
  const bytes = randomBytes(16);
  const millis = BigInt(Date.now());
  for (let index = 0; index < 6; index++) bytes[index] = Number((millis >> BigInt(40 - 8 * index)) & 0xffn);
  bytes[6] = (bytes[6] & 0x0f) | 0x70;
  bytes[8] = (bytes[8] & 0x3f) | 0x80;
  const hex = bytes.toString('hex');
  return [hex.slice(0, 8), hex.slice(8, 12), hex.slice(12, 16), hex.slice(16, 20), hex.slice(20)].join('-');
}

async function pageTarget() {
  // The WebKitGTK inspector serves one fixed page connection for the main view.
  if (webkit) {
    const deadline = Date.now() + 30000;
    while (Date.now() < deadline) {
      try {
        await fetch('http://127.0.0.1:' + port + '/');
        return { url: 'webkit', webSocketDebuggerUrl: 'ws://127.0.0.1:' + port + '/socket/1/1/WebPage' };
      } catch {}
      await new Promise((resolve) => setTimeout(resolve, 250));
    }
    throw new Error('No WebKit inspector on port ' + port);
  }
  const deadline = Date.now() + 30000;
  while (Date.now() < deadline) {
    try {
      const targets = await (await fetch('http://127.0.0.1:' + port + '/json')).json();
      const page = targets.find((target) => target.type === 'page' && target.webSocketDebuggerUrl);
      if (page) return page;
    } catch {}
    await new Promise((resolve) => setTimeout(resolve, 250));
  }
  throw new Error('No DevTools page target on port ' + port);
}

const page = await pageTarget();
const socket = new WebSocket(page.webSocketDebuggerUrl);
// Listeners are attached before the open completes so the first target event is not lost.
const opened = new Promise((resolve, reject) => {
  socket.onopen = resolve;
  socket.onerror = () => reject(new Error('DevTools connection failed'));
});
let nextId = 0;
const pending = new Map();
// WebKit multiplexes page commands through the Target domain.
let pageTargetId;
const targetReady = new Promise((resolve) => {
  socket.addEventListener('message', (message) => {
    const data = JSON.parse(message.data);
    if (data.method === 'Target.targetCreated' && data.params.targetInfo.type === 'page') {
      pageTargetId = data.params.targetInfo.targetId;
      resolve();
    }
  });
  if (!webkit) resolve();
});
socket.onmessage = (message) => {
  let data = JSON.parse(message.data);
  if (data.method === 'Target.dispatchMessageFromTarget') data = JSON.parse(data.params.message);
  pending.get(data.id)?.(data);
};
await opened;
async function send(method, params) {
  await targetReady;
  const id = ++nextId;
  return new Promise((resolve) => {
    pending.set(id, resolve);
    const command = JSON.stringify({ id, method, params });
    socket.send(webkit
      ? JSON.stringify({ id: ++nextId, method: 'Target.sendMessageToTarget', params: { targetId: pageTargetId, message: command } })
      : command);
  });
}

const out = { url: page.url, results: {} };
for (const command of commands) {
  const envelope = { name: command, contractVersion: 1, correlationId: uuidV7(), body: {} };
  const expression = '(async () => { const send = window.__diskunfoldRequest ?? ' +
    '((payload) => window.webkit.messageHandlers.diskunfold.postMessage(payload)); ' +
    'return await send(JSON.stringify(' + JSON.stringify({ command: 'du_call', args: { envelope } }) + ')); })()';
  let reply;
  if (webkit) {
    // WebKit resolves a promise with a separate command.
    const started = await send('Runtime.evaluate', { expression });
    reply = await send('Runtime.awaitPromise', {
      promiseObjectId: started.result.result.objectId,
      returnByValue: true,
    });
  } else {
    reply = await send('Runtime.evaluate', { expression, awaitPromise: true, returnByValue: true });
  }
  out.results[command] = reply.result?.result?.value !== undefined
    ? (typeof reply.result.result.value === 'string' ? JSON.parse(reply.result.result.value) : { nonStringValue: reply.result.result })
    : { evaluationError: reply.error ?? reply.result ?? reply };
}
socket.close();
console.log(JSON.stringify(out));
