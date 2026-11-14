const { readFileSync } = require('node:fs');
const { join } = require('node:path');
const { runInNewContext } = require('node:vm');
const { test } = require('node:test');
const assert = require('node:assert/strict');

function bootstrap() {
  // windows.rs embeds this file with include_str!("windows/bridge.js").
  const rust = readFileSync(join(__dirname, '../src/webview/windows.rs'), 'utf8');
  assert.match(rust, /const BRIDGE: &str = include_str!\("windows\/bridge\.js"\);/);
  const script = readFileSync(join(__dirname, '../src/webview/windows/bridge.js'), 'utf8');
  assert.ok(script);
  const handlers = {};
  const events = [];
  const posts = [];
  class Element {
    constructor(enabled) { this.enabled = enabled; }
    closest() { return this.enabled ? this : null; }
  }
  class CustomEvent {
    constructor(type, options) { this.type = type; this.detail = options.detail; }
  }
  const channel = {
    addEventListener(type, callback) { handlers[`channel:${type}`] = callback; },
    postMessageWithAdditionalObjects(message, files) { posts.push({ message, files }); },
  };
  const window = { dispatchEvent(event) { events.push(event); } };
  window.top = window;
  runInNewContext(script, {
    window, document: { addEventListener(type, callback) { handlers[type] = callback; } },
    chrome: { webview: channel }, location: { href: 'https://diskunfold.invalid/index.html' },
    Element, CustomEvent, setTimeout, clearTimeout,
  });
  return { handlers, events, posts, Element, channel };
}

function drop(bridge, trusted, enabled, items, files) {
  const event = { isTrusted: trusted, target: new bridge.Element(enabled), dataTransfer: { items, files }, preventDefault() { this.prevented = true; } };
  bridge.handlers.drop(event);
  assert.equal(event.prevented, true);
}

test('only trusted drops on an enabled zone reach the native bridge', () => {
  const bridge = bootstrap();
  const items = [{ kind: 'file' }];
  const files = [{}];
  drop(bridge, false, true, items, files);
  drop(bridge, true, false, items, files);
  assert.equal(bridge.posts.length, 0);
  drop(bridge, true, true, items, files);
  assert.equal(bridge.posts.length, 1);
  assert.equal(bridge.posts[0].message, 'du:folder-drop:1');
  assert.equal(bridge.posts[0].files, files);
  assert.equal(bridge.events[0].type, 'du://folder-drop');
  assert.equal(bridge.events[0].detail.seq, 1);
  assert.equal(bridge.events[0].detail.pending, true);
});

test('invalid and unsupported drops produce the specified reason', () => {
  const bridge = bootstrap();
  drop(bridge, true, true, [{ kind: 'file' }, { kind: 'file' }], [{}, {}]);
  assert.equal(bridge.events.at(-1).detail.reason, 'invalid_target');
  assert.equal(bridge.events.at(-1).detail.seq, 1);
  drop(bridge, true, true, [{ kind: 'file' }], []);
  assert.equal(bridge.events.at(-1).detail.reason, 'unsupported');
  assert.equal(bridge.events.at(-1).detail.seq, 2);
  delete bridge.channel.postMessageWithAdditionalObjects;
  drop(bridge, true, true, [{ kind: 'file' }], [{}]);
  assert.equal(bridge.events.at(-1).detail.reason, 'unsupported');
  assert.equal(bridge.events.at(-1).detail.seq, 3);
  assert.equal(bridge.posts.length, 0);
});

test('native path delivery preserves raw JavaScript UTF-16 units', () => {
  const bridge = bootstrap();
  drop(bridge, true, true, [{ kind: 'file' }], [{}]);
  bridge.handlers['channel:message']({ data: 'folder-drop:1\n{"path":"\\u0043\\u003a\\u005c\\ud800\\udc00\\udfff"}' });
  const { path, seq } = bridge.events.at(-1).detail;
  assert.equal(seq, 1);
  assert.deepEqual(Array.from({ length: path.length }, (_, i) => path.charCodeAt(i)), [67, 58, 92, 0xd800, 0xdc00, 0xdfff]);
});

test('native replies retain the drop sequence even when delivered out of order', () => {
  const bridge = bootstrap();
  drop(bridge, true, true, [{ kind: 'file' }], [{}]);
  drop(bridge, true, true, [{ kind: 'file' }], [{}]);
  const receive = bridge.handlers['channel:message'];
  receive({ data: 'folder-drop:2\n{"reason":"invalid_target"}' });
  receive({ data: 'folder-drop:1\n{"path":"old"}' });
  assert.deepEqual(bridge.posts.map(({ message }) => message), ['du:folder-drop:1', 'du:folder-drop:2']);
  assert.deepEqual(bridge.events.map(({ detail }) => detail.seq), [1, 2, 2, 1]);
  assert.equal(bridge.events[2].detail.reason, 'invalid_target');
  assert.equal(bridge.events[3].detail.path, 'old');
});

test('a single-item request cannot smuggle multiple files to the native channel', () => {
  const bridge = bootstrap();
  drop(bridge, true, true, [{ kind: 'file' }], [{}, {}]);
  assert.equal(bridge.posts.length, 0);
  assert.deepEqual(bridge.events.map(({ detail }) => [detail.seq, detail.pending ?? detail.reason]), [
    [1, true], [1, 'invalid_target'],
  ]);
});

test('additional-object transport failure closes exactly its own request', () => {
  const bridge = bootstrap();
  const calls = [];
  bridge.channel.postMessageWithAdditionalObjects = (message) => {
    calls.push(message);
    if (calls.length === 1) throw new Error('transport failure');
  };
  drop(bridge, true, true, [{ kind: 'file' }], [{}]);
  drop(bridge, true, true, [{ kind: 'file' }], [{}]);
  bridge.handlers['channel:message']({ data: 'folder-drop:2\n{"path":"C:\\\\new"}' });
  assert.deepEqual(calls, ['du:folder-drop:1', 'du:folder-drop:2']);
  assert.deepEqual(bridge.events.map(({ detail }) => [detail.seq, detail.pending ?? detail.reason ?? detail.path]), [
    [1, true], [1, 'unsupported'], [2, true], [2, 'C:\\new'],
  ]);
});
