import assert from 'node:assert/strict';
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import test from 'node:test';
import { findWar, javaCandidates, javaMajor } from './view-architecture-docs.mjs';

test('HS2-38CA53 javaMajor reads modern and legacy version strings', () => {
  assert.equal(javaMajor('openjdk version "27.0.1.1" 2026-11-17'), 27);
  assert.equal(javaMajor('openjdk version "21" 2023-09-19'), 21);
  assert.equal(javaMajor('java version "1.8.0_401"'), 8);
  assert.equal(javaMajor('Unable to locate a Java Runtime.'), null);
});

test('HS2-38CA53 javaCandidates prefers JAVA_HOME, then existing Homebrew kegs, then PATH', () => {
  const present = new Set(['/jdk/bin/java', '/opt/homebrew/opt/openjdk/bin/java']);
  const list = javaCandidates({ JAVA_HOME: '/jdk' }, (path) => present.has(path));
  assert.deepEqual(list, ['/jdk/bin/java', '/opt/homebrew/opt/openjdk/bin/java', 'java']);
  assert.deepEqual(
    javaCandidates({}, () => false),
    ['java'],
  );
});

test('HS2-38CA53 findWar picks the largest war above the size floor', (t) => {
  const dir = mkdtempSync(join(tmpdir(), 'hs2-38ca53-'));
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  mkdirSync(join(dir, 'app', 'target'), { recursive: true });
  writeFileSync(join(dir, 'app', 'target', 'small.war'), Buffer.alloc(10));
  writeFileSync(join(dir, 'app', 'target', 'big.war'), Buffer.alloc(200));
  writeFileSync(join(dir, 'app', 'target', 'big.jar'), Buffer.alloc(500));
  assert.equal(findWar(dir, 100), join(dir, 'app', 'target', 'big.war'));
  assert.equal(findWar(dir, 1000), null);
  assert.equal(findWar(join(dir, 'missing')), null);
});
