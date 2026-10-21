// Optional real-tool acceptance check. Run through pinned Rulesync's npm exec PATH.
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';

const root = path.resolve(import.meta.dirname, '..');
const cli = path.join(root, 'target/debug/hotsheet-cli');
const example = path.join(root, 'guidance/rulesync');
const config = JSON.parse(fs.readFileSync(path.join(example, 'rulesync.jsonc'), 'utf8'));
const pinnedCommit = JSON.parse(fs.readFileSync(path.join(root, 'guidance/ai-playbook.lock.json'), 'utf8')).commit;
const temporary = fs.mkdtempSync(path.join(os.tmpdir(), 'hotsheet-rulesync-'));
const env = { ...process.env, HOTSHEET_HOME: path.join(temporary, 'home') };
const write = (file, text) => {
  fs.mkdirSync(path.dirname(file), { recursive: true });
  fs.writeFileSync(file, text);
};
const read = (project, file = 'AGENTS.md') => fs.readFileSync(path.join(project, file), 'utf8');
const count = (text, value) => text.split(value).length - 1;
const localRule = (text) => `---\nroot: true\ntargets: ["*"]\n---\n\n${text}\n`;
function run(program, args, cwd, expected = 0) {
  const result = spawnSync(program, args, { cwd, env, encoding: 'utf8', timeout: 120_000 });
  assert.equal(
    result.status,
    expected,
    `${program} ${args.join(' ')}\n${result.error ?? ''}\n${result.stdout}\n${result.stderr}`,
  );
  return result.stdout;
}
const rulesync = (project, ...args) => run('rulesync', args, project);
let store;
const hs = (project, ...args) => run(cli, ['-C', store, 'setup', ...args, '--project', project], root);
function project(name, sourceConfig = config, policy = 'Local project policy.') {
  const directory = path.join(temporary, name);
  write(path.join(directory, 'rulesync.jsonc'), `${JSON.stringify(sourceConfig, null, 2)}\n`);
  write(path.join(directory, '.rulesync/rules/project.md'), localRule(policy));
  write(path.join(directory, '.hotsheet2/settings.json'), JSON.stringify({ enabled_plugins: ['claude', 'codex'] }));
  write(path.join(directory, 'CLAUDE.md'), '@AGENTS.md\n');
  return directory;
}
try {
  assert.equal(run('rulesync', ['--version'], root).trim(), '38.0.0');
  store = path.join(temporary, 'tickets.hs2');
  fs.mkdirSync(store);
  run(cli, ['-C', store, 'init', '--prefix', 'HS'], root);

  const fresh = project('fresh');
  hs(fresh, '--guidance-owner', 'rulesync');
  const exampleLock = path.join(example, 'rulesync.lock');
  if (fs.existsSync(exampleLock)) {
    fs.copyFileSync(exampleLock, path.join(fresh, 'rulesync.lock'));
    rulesync(fresh, 'install', '--frozen');
  } else {
    assert(process.argv.includes('--write-lock'), 'Generate the initial example lock with --write-lock');
    rulesync(fresh, 'install');
  }
  rulesync(fresh, 'generate');
  rulesync(fresh, 'generate', '--check');
  const generated = read(fresh);
  assert.equal(count(generated, '## Hot Sheet — ticket workflow'), 1);
  assert(generated.includes('Local project policy.'));
  assert.equal(read(fresh, 'CLAUDE.md'), '@AGENTS.md\n');
  for (const tool of ['claude', 'codex']) hs(fresh, tool);
  hs(fresh, '--refresh');
  hs(fresh, '--refresh');
  assert.equal(read(fresh), generated);
  assert(hs(fresh, '--diagnose', '--tool', 'claude', '--json').includes('managed by Rulesync'));
  const locked = JSON.parse(read(fresh, 'rulesync.lock'));
  assert(Object.values(locked.sources).some((source) => source.resolvedRef === pinnedCommit));
  if (process.argv.includes('--write-lock')) fs.copyFileSync(path.join(fresh, 'rulesync.lock'), exampleLock);

  // Reproduce the documented existing-project migration, including manual local extraction.
  const existing = project('existing', config, 'Existing project policy.\nLocal runner: npm test.');
  hs(existing, 'claude');
  hs(existing, 'codex');
  const specifics =
    '<!-- hotsheet:begin specifics=testing-philosophy v=1 -->\nLocal runner: npm test.\n<!-- hotsheet:end specifics=testing-philosophy -->\n\n';
  write(
    path.join(existing, 'AGENTS.md'),
    `Existing project policy.\n${read(existing).replace('<!-- hotsheet:end section=testing-philosophy -->', `${specifics}<!-- hotsheet:end section=testing-philosophy -->`)}`,
  );
  const before = read(existing);
  write(path.join(existing, '.hotsheet2/guidance-backup/AGENTS.md'), before);
  hs(existing, '--guidance-owner', 'rulesync');
  const selected = structuredClone(config);
  selected.sources[0].rules = ['hotsheet-ticket-driven-work', 'hotsheet-testing-philosophy'];
  write(path.join(existing, 'rulesync.jsonc'), JSON.stringify(selected));
  rulesync(existing, 'install');
  const beforePreview = read(existing);
  rulesync(existing, 'generate', '--dry-run');
  assert.equal(read(existing), beforePreview);
  rulesync(existing, 'generate');
  let text = read(existing);
  assert(text.includes('Existing project policy.') && text.includes('Local runner: npm test.'));
  assert.equal(count(text, '<!-- hotsheet:begin section=ticket-driven-work'), 1);
  assert(!text.includes('<!-- hotsheet:begin section=visual-qa'));
  assert(!text.includes('## Hot Sheet — ticket workflow'));
  // Changing selection installs at the same locked ref and does not lose local rules.
  write(path.join(existing, 'rulesync.jsonc'), JSON.stringify(config));
  rulesync(existing, 'install');
  rulesync(existing, 'generate');
  text = read(existing);
  assert.equal(count(text, '## Hot Sheet — ticket workflow'), 1);
  assert(text.includes('Local runner: npm test.'));
  hs(existing, '--refresh');
  assert.equal(read(existing), text);
  // The documented return starts from reviewed local-only content, then re-enables built-in ownership.
  write(path.join(existing, 'AGENTS.md'), 'Existing project policy.\nLocal runner: npm test.\nFurther local edit.\n');
  hs(existing, '--guidance-owner', 'builtin');
  for (const tool of ['claude', 'codex']) hs(existing, tool);
  hs(existing, '--refresh');
  text = read(existing);
  assert(text.includes('Further local edit.'));
  assert.equal(count(text, '## Hot Sheet — ticket workflow'), 1);

  // Exercise an actual source revision update through a local Git transport.
  const upstream = path.join(temporary, 'upstream');
  run('git', ['clone', '--quiet', '--branch', config.sources[0].ref, config.sources[0].source, upstream], temporary);
  run('git', ['switch', '--quiet', '-c', 'validation-updates'], upstream);
  const moving = structuredClone(config);
  moving.sources[0] = {
    source: `file://${upstream}`,
    transport: 'git',
    ref: 'validation-updates',
    rules: ['hotsheet-ticket-driven-work'],
  };
  const updates = project('updates', moving, 'Local customization survives updates.');
  rulesync(updates, 'install');
  rulesync(updates, 'generate');
  fs.appendFileSync(path.join(upstream, 'rules/hotsheet-ticket-driven-work.md'), '\nUpdated upstream fixture.\n');
  run('git', ['add', 'rules/hotsheet-ticket-driven-work.md'], upstream);
  run(
    'git',
    [
      '-c',
      'user.name=Rulesync validation',
      '-c',
      'user.email=validation@example.invalid',
      'commit',
      '--quiet',
      '-m',
      'fixture update',
    ],
    upstream,
  );
  run('rulesync', ['install', '--outdated'], updates, 1);
  rulesync(updates, 'install', '--frozen');
  rulesync(updates, 'generate');
  assert(!read(updates).includes('Updated upstream fixture.'));
  rulesync(updates, 'install', '--update');
  rulesync(updates, 'generate');
  assert(read(updates).includes('Updated upstream fixture.'));
  assert(read(updates).includes('Local customization survives updates.'));
  write(
    path.join(updates, '.rulesync/rules/hotsheet-ticket-driven-work.md'),
    localRule('Reviewed local rule override.'),
  );
  rulesync(updates, 'install', '--update');
  rulesync(updates, 'generate');
  assert(read(updates).includes('Reviewed local rule override.'));
  assert(!read(updates).includes('Updated upstream fixture.'));
  console.log(
    'Rulesync 38 acceptance passed: fresh, existing, selected/all, frozen/update/outdated, local override, and built-in return.',
  );
} finally {
  fs.rmSync(temporary, { recursive: true, force: true });
}
