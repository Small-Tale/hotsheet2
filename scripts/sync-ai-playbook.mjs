import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

export const repository = 'https://github.com/Small-Tale/ai-playbook.git';
export const destinations = {
  'hotsheet-ticket-driven-work': ['plugins/shared/ticket-driven-work.md'],
  'hotsheet-testing-philosophy': ['plugins/shared/testing-philosophy.md'],
  'hotsheet-requirements-documentation': ['plugins/shared/requirements-documentation.md'],
  'hotsheet-visual-qa': ['plugins/shared/visual-qa.md'],
  'hotsheet-workflow': ['claude', 'codex', 'antigravity', 'opencode'].map((tool) => `plugins/${tool}/instructions.md`),
};
const digest = (text) => createHash('sha256').update(text).digest('hex');

export function ruleBody(text) {
  const match = text.match(/^---\n[\s\S]*?\n---\n\n/);
  if (!match) throw new Error('Rulesync rule frontmatter is missing');
  return text.slice(match[0].length);
}

export function validateLock(lock) {
  if (lock.schema_version !== 1 || lock.repository !== repository || !/^[a-f0-9]{40}$/.test(lock.commit))
    throw new Error('Invalid ai-playbook provenance');
  if (lock.rules.length !== Object.keys(destinations).length) throw new Error('Incomplete rule snapshot');
  const names = new Set();
  for (const rule of lock.rules) {
    if (
      names.has(rule.name) ||
      !destinations[rule.name] ||
      rule.path !== `rules/${rule.name}.md` ||
      !Number.isInteger(rule.version) ||
      rule.version < 1 ||
      !/^[a-f0-9]{64}$/.test(rule.sha256)
    )
      throw new Error(`Invalid snapshot rule: ${rule.name}`);
    names.add(rule.name);
  }
}

export function checkBundled(root, lock) {
  validateLock(lock);
  for (const rule of lock.rules) {
    for (const destination of destinations[rule.name]) {
      if (digest(fs.readFileSync(path.join(root, destination), 'utf8')) !== rule.sha256)
        throw new Error(`Guidance drift: ${destination}; update from the pinned ai-playbook source`);
    }
  }
}

function git(source, ...args) {
  return execFileSync('git', ['-C', source, ...args], { encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] });
}

export function readSource(source, commit, release) {
  if (!/^[a-f0-9]{40}$/.test(commit)) throw new Error('Use a full immutable source commit SHA');
  const manifest = JSON.parse(git(source, 'show', `${commit}:rules-manifest.json`));
  if (manifest.schema_version !== 1) throw new Error('Unsupported upstream rule manifest');
  const rules = manifest.rules.map(({ name, path: rulePath, version, sha256 }) => ({
    name,
    path: rulePath,
    version,
    sha256,
  }));
  const lock = { schema_version: 1, repository, commit, release, rules };
  validateLock(lock);
  const bodies = new Map();
  for (const rule of rules) {
    const body = ruleBody(git(source, 'show', `${commit}:${rule.path}`));
    if (digest(body) !== rule.sha256) throw new Error(`Upstream hash mismatch: ${rule.name}`);
    const section = rule.name.slice('hotsheet-'.length);
    const marker =
      section === 'workflow'
        ? `<!-- hotsheet-instructions-version: ${rule.version} -->`
        : `<!-- hotsheet:begin section=${section} v=${rule.version} -->`;
    if (!body.startsWith(`${marker}\n`)) throw new Error(`Upstream version mismatch: ${rule.name}`);
    bodies.set(rule.name, body);
  }
  return { lock, bodies };
}

export function main(args, root = path.resolve(import.meta.dirname, '..')) {
  let source,
    commit,
    release,
    write = false,
    fetch = false;
  for (let index = 0; index < args.length; index += 1) {
    const arg = args[index];
    if (arg === '--write') write = true;
    else if (arg === '--check') continue;
    else if (arg === '--fetch') fetch = true;
    else if (['--source', '--ref', '--release', '--root'].includes(arg)) {
      const value = args[++index];
      if (!value) throw new Error(`Missing value for ${arg}`);
      if (arg === '--source') source = path.resolve(value);
      else if (arg === '--ref') commit = value;
      else if (arg === '--release') release = value;
      else root = path.resolve(value);
    } else throw new Error(`Unknown argument: ${arg}`);
  }
  if (source && fetch) throw new Error('Choose --source or --fetch');
  if (write && (!source || !commit || !release)) throw new Error('--write requires --source, --ref, and --release');
  const lockPath = path.join(root, 'guidance/ai-playbook.lock.json');
  const current = write ? undefined : JSON.parse(fs.readFileSync(lockPath, 'utf8'));
  if (current) validateLock(current);
  let temporary;
  try {
    if (fetch) {
      temporary = fs.mkdtempSync(path.join(os.tmpdir(), 'hotsheet-playbook-'));
      source = temporary;
      execFileSync('git', ['init', '--quiet', source]);
      execFileSync('git', ['-C', source, 'fetch', '--quiet', '--depth=1', repository, current.commit]);
    }
    if (source) {
      const upstream = readSource(source, commit ?? current.commit, release ?? current.release);
      if (write) {
        for (const [name, body] of upstream.bodies)
          for (const destination of destinations[name]) fs.writeFileSync(path.join(root, destination), body);
        fs.mkdirSync(path.dirname(lockPath), { recursive: true });
        fs.writeFileSync(lockPath, `${JSON.stringify(upstream.lock, null, 2)}\n`);
      } else if (JSON.stringify(upstream.lock) !== JSON.stringify(current)) {
        throw new Error('Pinned upstream source differs from the bundled provenance manifest');
      }
      checkBundled(root, upstream.lock);
    } else checkBundled(root, current);
    console.log(`Verified ${Object.keys(destinations).length} pinned ai-playbook rules`);
  } finally {
    if (temporary) fs.rmSync(temporary, { recursive: true, force: true });
  }
}

if (process.argv[1] && fileURLToPath(import.meta.url) === path.resolve(process.argv[1])) {
  try {
    main(process.argv.slice(2));
  } catch (error) {
    console.error(error.message);
    process.exitCode = 1;
  }
}
