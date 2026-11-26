// HS2-DHCY18: conventions that keep the Structurizr workspace readable as a
// guided tour. See docs/architecture/README.md "Model conventions".
import assert from 'node:assert/strict';
import { existsSync, readdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';

const ARCH = fileURLToPath(new URL('../docs/architecture/', import.meta.url));
const read = (path) => readFileSync(join(ARCH, path), 'utf8');
const viewFiles = readdirSync(join(ARCH, 'views')).filter((name) => name.endsWith('.dsl'));
const VIEW_START =
  /^\s*(component|dynamic|container|systemContext|systemLandscape|deployment|custom|filtered|image)\s+(?:\S+\s+)?(?:"[^"]*"\s+)?"(HS2-[A-Za-z0-9-]+)"/;

function views() {
  const out = [];
  for (const file of viewFiles) {
    const lines = read(join('views', file)).split('\n');
    lines.forEach((line, index) => {
      const match = VIEW_START.exec(line);
      if (match) out.push({ file, kind: match[1], key: match[2], line, next: lines[index + 1] ?? '' });
    });
  }
  return out;
}

test('HS2-DHCY18 every view has a layer-numbered title', () => {
  const all = views();
  assert.ok(all.length > 50, `expected the full view set, found ${all.length}`);
  for (const view of all) {
    const title = /^\s*title "(\d) [^"]+"/.exec(view.next) ?? /"(\d) · [^"]+"/.exec(view.line);
    assert.ok(title, `${view.file}: ${view.key} needs title "<layer> <Area> · <name>"`);
  }
});

test('HS2-DHCY18 views are listed in tour order with the overview first', () => {
  const workspace = read('workspace.dsl');
  assert.match(workspace, /"structurizr\.sort"\s+"created"/);
  const order = [...workspace.matchAll(/!include views\/([a-z-]+)\.dsl/g)].map((m) => m[1]);
  assert.deepEqual(order, [
    'tour',
    'shared',
    'web-client',
    'web-host',
    'server',
    'cli',
    'core',
    'orchestration',
    'flows',
    'screenshots',
  ]);
  const withDefault = viewFiles.filter((file) => /^\s*default\s*$/m.test(read(join('views', file))));
  assert.deepEqual(withDefault, ['tour.dsl']);
});

test('HS2-DHCY18 overview elements open an existing detailed view', () => {
  const keys = new Set(views().map((view) => view.key));
  const overview = read('model/overview.dsl');
  const elements = [...overview.matchAll(/^(ov[A-Za-z]+) = element /gm)].map((m) => m[1]);
  assert.ok(elements.length >= 6);
  const urls = [...overview.matchAll(/url "\{workspace\}\/diagrams#([^"]+)"/g)].map((m) => m[1]);
  assert.equal(urls.length, elements.length, 'every overview element needs a {workspace}/diagrams# url');
  for (const key of urls) assert.ok(keys.has(key), `overview url names unknown view ${key}`);
  const tour = read('views/tour.dsl');
  for (const id of elements) assert.match(tour, new RegExp(`\\b${id}\\b`), `${id} missing from HS2-00-Overview`);
});

test('HS2-DHCY18 no style tag is set both outside and inside a light or dark block', () => {
  // The viewer merges same-tag style definitions into the first one in place,
  // so mixing scopes for one tag leaks dark colours into light mode.
  const workspace = read('workspace.dsl');
  const styles = workspace.slice(workspace.indexOf('styles {'));
  const stack = [];
  const scoped = new Set();
  const unscoped = new Set();
  for (const raw of styles.split('\n')) {
    const line = raw.trim();
    if (line.endsWith('{')) {
      const tag = /^(element|relationship) "([^"]+)"/.exec(line);
      if (tag) (stack.includes('light') || stack.includes('dark') ? scoped : unscoped).add(tag[2]);
      stack.push(line.replace(/\s*\{$/, ''));
    } else if (line === '}') {
      stack.pop();
      if (stack.length === 0) break;
    }
  }
  assert.ok(scoped.size > 0, 'expected light and dark style blocks');
  for (const tag of unscoped) assert.ok(!scoped.has(tag), `style tag "${tag}" is set in and out of a colour scheme`);
});

test('HS2-DHCY18 narrative chapters start at ## and images exist', () => {
  for (const file of readdirSync(join(ARCH, 'narrative')).filter((name) => name.endsWith('.md'))) {
    let fenced = false;
    for (const line of read(join('narrative', file)).split('\n')) {
      if (line.startsWith('```')) fenced = !fenced;
      if (!fenced) assert.doesNotMatch(line, /^# /, `${file}: the documentation viewer drops # headings`);
      for (const [, src] of line.matchAll(/!\[[^\]]*\]\(([^)]+)\)/g)) {
        if (src.startsWith('embed:')) continue;
        assert.ok(existsSync(join(ARCH, 'narrative', src)), `${file}: missing image ${src}`);
      }
    }
  }
});
