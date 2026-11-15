#!/usr/bin/env node
// Rulesync regeneration check (HS2-VA97HW). The committed AGENTS.md must equal a fresh run of the
// pinned Rulesync over the same inputs Hot Sheet's guidance apply materializes: the selected rules at
// the pinned ai-playbook commit (`.rulesync/rules/.curated/`), the bundled Hot Sheet sections no
// selected rule carries (`plugins/shared/*.md`, HS2-6FSJCV), the committed local rule, and the
// generated `agentsmd` rules config. Hand-edited or stale instruction output fails here.
//
//   node scripts/check-rulesync-generation.mjs --fetch                 # CI: fetch the pinned commit
//   node scripts/check-rulesync-generation.mjs --source ../ai-playbook # local checkout holding it
//
// Run `npm ci` at the repository root first; Rulesync is pinned there.

import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

import { withoutGitRepositoryEnv } from '../clients/web/scripts/repository-env.mjs';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const SELECTION = 'guidance/rulesync/project-selection.json';
// Byte-identical to the config `hotsheet-cli guidance apply` writes (guidance_sources.rs).
const GENERATED_CONFIG = `${JSON.stringify({ features: ['rules'], targets: ['agentsmd'] }, null, 2)}`;

export function readSelection(text) {
  const selection = JSON.parse(text);
  if (!/^[a-f0-9]{40}$/.test(selection.commit ?? '')) throw new Error('Selection must pin a full source commit');
  if (!Array.isArray(selection.rules) || selection.rules.length === 0) throw new Error('Selection names no rules');
  for (const name of selection.rules)
    if (!/^[a-z0-9-]+$/.test(name)) throw new Error(`Invalid selected rule name: ${String(name)}`);
  if (new Set(selection.rules).size !== selection.rules.length) throw new Error('Selection repeats a rule');
  return selection;
}

/**
 * Bundled Hot Sheet sections a Rulesync project receives as generated rule inputs (HS2-6FSJCV), as
 * `[stem, content]` pairs. Mirrors `rulesync_bundled_sections` in crates/hotsheet-plugins/src/setup.rs:
 * every current (non-legacy) `plugins/shared/*.md` section whose marker no selected rule carries.
 */
export function bundledSections(selectedRuleTexts, shared = path.join(root, 'plugins/shared')) {
  return fs
    .readdirSync(shared)
    .filter((file) => file.endsWith('.md') && !file.startsWith('legacy-'))
    .sort()
    .map((file) => {
      const body = fs.readFileSync(path.join(shared, file), 'utf8').trim();
      const name = /^<!-- hotsheet:begin section=([a-z0-9-]+) v=\d+ -->/.exec(body)?.[1];
      if (name === undefined) throw new Error(`plugins/shared/${file} does not open with a section marker`);
      return { name, body };
    })
    .filter(({ name }) => !selectedRuleTexts.some((text) => text.includes(`<!-- hotsheet:begin section=${name} v=`)))
    .map(({ name, body }) => [
      `hotsheet-bundled-${name}`,
      `---\nroot: true\ntargets: ["*"]\ndescription: "Hot Sheet bundled ${name} guidance"\n---\n\n${body}\n`,
    ]);
}

/** Lay out a Rulesync project exactly as guidance apply does, without touching the repository. */
export function materialize(project, { rules, readRule, localRule, shared }) {
  const curated = path.join(project, '.rulesync/rules/.curated');
  fs.mkdirSync(curated, { recursive: true });
  const texts = rules.map((name) => readRule(name));
  rules.forEach((name, index) => fs.writeFileSync(path.join(curated, `${name}.md`), texts[index]));
  for (const [stem, content] of bundledSections(texts, shared))
    fs.writeFileSync(path.join(curated, `${stem}.md`), content);
  fs.writeFileSync(path.join(project, '.rulesync/rules/project-local.md'), localRule);
  fs.mkdirSync(path.join(project, '.hotsheet2'), { recursive: true });
  fs.writeFileSync(path.join(project, '.hotsheet2/rulesync.generated.jsonc'), GENERATED_CONFIG);
  execFileSync('git', ['init', '--quiet', project], { env: withoutGitRepositoryEnv(process.env) });
}

/** Run the pinned Rulesync in `project` and return the AGENTS.md it wrote. */
export function generate(project, rulesync = path.join(root, 'node_modules/.bin/rulesync')) {
  if (!fs.existsSync(rulesync)) throw new Error('Pinned Rulesync is missing; run npm ci at the repository root.');
  execFileSync(rulesync, ['generate', '--delete', '--config', '.hotsheet2/rulesync.generated.jsonc'], {
    cwd: project,
    env: withoutGitRepositoryEnv(process.env),
    stdio: ['ignore', 'pipe', 'pipe'],
  });
  return fs.readFileSync(path.join(project, 'AGENTS.md'), 'utf8');
}

/** The first differing line, or undefined when the committed output equals the regeneration. */
export function firstDifference(committed, generated) {
  if (committed === generated) return undefined;
  const left = committed.split('\n'),
    right = generated.split('\n');
  for (let index = 0; index < Math.max(left.length, right.length); index += 1)
    if (left[index] !== right[index])
      return { line: index + 1, committed: left[index] ?? '<end of file>', generated: right[index] ?? '<end of file>' };
  return { line: left.length, committed: '<trailing bytes differ>', generated: '<trailing bytes differ>' };
}

function sourceCheckout(args, selection) {
  const at = args.indexOf('--source');
  if (at !== -1) return { directory: path.resolve(args[at + 1] ?? ''), temporary: false };
  if (!args.includes('--fetch')) throw new Error('Choose --fetch or --source <ai-playbook checkout>');
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'hotsheet-rulesync-source-'));
  const env = withoutGitRepositoryEnv(process.env);
  execFileSync('git', ['init', '--quiet', directory], { env });
  execFileSync('git', ['-C', directory, 'fetch', '--quiet', '--depth=1', selection.repository, selection.commit], {
    env,
  });
  return { directory, temporary: true };
}

export function main(args) {
  const selection = readSelection(fs.readFileSync(path.join(root, SELECTION), 'utf8'));
  const source = sourceCheckout(args, selection);
  const project = fs.mkdtempSync(path.join(os.tmpdir(), 'hotsheet-rulesync-project-'));
  try {
    materialize(project, {
      rules: selection.rules,
      readRule: (name) =>
        execFileSync('git', ['-C', source.directory, 'show', `${selection.commit}:rules/${name}.md`], {
          encoding: 'utf8',
          env: withoutGitRepositoryEnv(process.env),
          stdio: ['ignore', 'pipe', 'pipe'],
        }),
      localRule: fs.readFileSync(path.join(root, '.rulesync/rules/project-local.md'), 'utf8'),
    });
    const difference = firstDifference(fs.readFileSync(path.join(root, 'AGENTS.md'), 'utf8'), generate(project));
    if (difference) {
      throw new Error(
        `AGENTS.md differs from the pinned Rulesync regeneration at line ${difference.line}:\n` +
          `  committed: ${difference.committed}\n  generated: ${difference.generated}\n` +
          'Edit .rulesync/rules/project-local.md or the selection, then regenerate; never hand-edit AGENTS.md.',
      );
    }
    console.log(
      `AGENTS.md equals the Rulesync regeneration of ${selection.rules.length} pinned rules + bundled sections + local rule.`,
    );
  } finally {
    fs.rmSync(project, { recursive: true, force: true });
    if (source.temporary) fs.rmSync(source.directory, { recursive: true, force: true });
  }
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  try {
    main(process.argv.slice(2));
  } catch (error) {
    console.error(error instanceof Error ? error.message : String(error));
    process.exitCode = 1;
  }
}
