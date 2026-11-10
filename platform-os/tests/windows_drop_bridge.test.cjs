const { readFileSync } = require('node:fs');
const { join } = require('node:path');
const { runInNewContext } = require('node:vm');
const { test } = require('node:test');
const assert = require('node:assert/strict');

function bootstrap() {
  const rust = readFileSync(join(__dirname, '../src/webview/windows.rs'), 'utf8');
  const script = rust.match(/const BRIDGE: &str = r#"([\s\S]*?)"#;/)?.[1];
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
  assert.equal(bridge.posts[0].message, 'du:folder-drop');
  assert.equal(bridge.posts[0].files, files);
});

test('invalid and unsupported drops produce the specified reason', () => {
  const bridge = bootstrap();
  drop(bridge, true, true, [{ kind: 'file' }, { kind: 'file' }], [{}, {}]);
  assert.equal(bridge.events.at(-1).detail.reason, 'invalid_target');
  drop(bridge, true, true, [{ kind: 'file' }], []);
  assert.equal(bridge.events.at(-1).detail.reason, 'unsupported');
  delete bridge.channel.postMessageWithAdditionalObjects;
  drop(bridge, true, true, [{ kind: 'file' }], [{}]);
  assert.equal(bridge.events.at(-1).detail.reason, 'unsupported');
});

test('native path delivery preserves raw JavaScript UTF-16 units', () => {
  const bridge = bootstrap();
  bridge.handlers['channel:message']({ data: 'folder-drop\n{"path":"\\u0043\\u003a\\u005c\\ud800\\udc00\\udfff"}' });
  const path = bridge.events.at(-1).detail.path;
  assert.deepEqual(Array.from({ length: path.length }, (_, i) => path.charCodeAt(i)), [67, 58, 92, 0xd800, 0xdc00, 0xdfff]);
});
