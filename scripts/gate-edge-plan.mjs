// HS2-55MWQ9: source-proven candidate edges for the required gate's read-only plan.
// HS2-DAA8AB still owns validation before any candidate can replace a required stage.
import { spawnSync } from 'node:child_process';
import { existsSync, readFileSync } from 'node:fs';
import { join } from 'node:path';

const RUST_PRIVATE_PATH = 'crates/hotsheet-cli/src/broken_pipe.rs';
const WEB_PRIVATE_PATH = 'clients/web/src/trailing-task-internal.ts';
const WEB_PUBLIC_PATH = 'clients/web/src/trailing-task.ts';
const RUST_PRIVATE_START = 'fn mentions_broken_pipe(message: &str) -> bool {\n';
const RUST_PRIVATE_END = '\n}\n\n/// Install a panic hook';

function splitPrivateRustBody(source) {
  const start = source.indexOf(RUST_PRIVATE_START);
  if (start < 0 || source.indexOf(RUST_PRIVATE_START, start + 1) >= 0) return null;
  const bodyStart = start + RUST_PRIVATE_START.length;
  const end = source.indexOf(RUST_PRIVATE_END, bodyStart);
  if (end < 0 || source.indexOf(RUST_PRIVATE_END, end + 1) >= 0) return null;
  return { outside: source.slice(0, bodyStart) + source.slice(end), body: source.slice(bodyStart, end) };
}

export function rustPrivateBodyOnly(before, after) {
  const oldParts = splitPrivateRustBody(before);
  const newParts = splitPrivateRustBody(after);
  return Boolean(oldParts && newParts && oldParts.outside === newParts.outside && oldParts.body !== newParts.body);
}

/** Use Cargo's current regular dependency graph, not a copied package list. */
export function rustConsumersFromMetadata(metadata, crate) {
  const members = new Set(metadata.workspace_members);
  const packages = metadata.packages.filter((item) => members.has(item.id));
  const names = new Set(packages.map((item) => item.name));
  if (!names.has(crate) || names.size !== 14) return null;
  const reverse = new Map([...names].map((name) => [name, new Set()]));
  for (const pkg of packages) {
    for (const dep of pkg.dependencies) {
      if (dep.kind !== null || !dep.name.startsWith('hotsheet-')) continue;
      if (!names.has(dep.name)) return null;
      reverse.get(dep.name).add(pkg.name);
    }
  }
  const reached = new Set([crate]);
  const queue = [crate];
  while (queue.length) {
    for (const consumer of reverse.get(queue.shift()) ?? []) {
      if (reached.has(consumer)) continue;
      reached.add(consumer);
      queue.push(consumer);
    }
  }
  return [...reached].sort();
}

function stage(cwd, command, reason) {
  return { cwd, command, reason };
}

const broad = (reason, rustConsumers = []) => ({
  mode: 'broad',
  reason,
  rustConsumers,
  suggestedStages: [],
  requiredGate: 'unchanged pending HS2-DAA8AB',
});

/** Pure candidate selection: a mixed or unproved diff never gains a narrow stage. */
export function edgePreview({ changes, rustBefore, rustAfter, rustConsumers = [], webBoundaryReady = false }) {
  if (changes.length !== 1)
    return broad('Only one exact private pilot path has a proven test boundary.', rustConsumers);
  const [path] = changes;
  if (path === WEB_PRIVATE_PATH) {
    if (!webBoundaryReady) return broad('The web public-import boundary or its contract tests are missing.');
    return {
      mode: 'candidate',
      reason: 'HS2-K5E2KF pins the private timer implementation behind the public scheduler API.',
      rustConsumers: [],
      suggestedStages: [
        stage(
          'clients/web',
          'npx vitest run src/trailing-task.test.ts src/eslint-policy.test.ts',
          'public API and timer transitions',
        ),
        stage(
          'clients/web',
          "npx playwright test tests/providers.spec.ts --grep 'defers ticket refresh without hiding an open metadata popup'",
          'shipped application consumer',
        ),
      ],
      requiredGate: 'unchanged pending HS2-DAA8AB',
    };
  }
  if (path === RUST_PRIVATE_PATH) {
    if (!rustBefore || !rustAfter || !rustPrivateBodyOnly(rustBefore, rustAfter))
      return broad('Broken-pipe edit reaches a public signature or cannot be proved private.', rustConsumers);
    if (
      !rustConsumers.includes('hotsheet-cli') ||
      !rustConsumers.includes('hotsheet-mcp') ||
      !rustConsumers.includes('hotsheet-server')
    )
      return broad('Cargo consumer graph is missing a required CLI consumer.', rustConsumers);
    return {
      mode: 'candidate',
      reason: 'Only the private broken-pipe parser body changed; Cargo metadata supplies reverse consumers.',
      rustConsumers,
      suggestedStages: [
        stage(
          '.',
          `cargo check --all-features ${rustConsumers.map((name) => `-p ${name}`).join(' ')}`,
          'compile all reverse Cargo consumers',
        ),
        stage(
          '.',
          "cargo nextest run -p hotsheet-cli -E 'test(broken_pipe)' --status-level leak",
          'direct parser unit tests',
        ),
        stage(
          '.',
          'cargo nextest run -p hotsheet-cli --test broken_pipe --status-level leak',
          'CLI pipe composition test',
        ),
      ],
      requiredGate: 'unchanged pending HS2-DAA8AB',
    };
  }
  return broad(
    'Public, generated, shared, cross-package, or unknown input keeps the existing affected gate.',
    rustConsumers,
  );
}

function command(repo, program, args) {
  const result = spawnSync(program, args, { cwd: repo, encoding: 'utf8', maxBuffer: 16 * 1024 * 1024 });
  return result.status === 0 ? result.stdout : null;
}

/** Resolve proof inputs from the exact checkout. Any failed read returns a broad plan. */
export function edgePreviewForRepo(repo, base, changes) {
  const path = changes.length === 1 ? changes[0] : null;
  let rustConsumers = [];
  const crate = /^crates\/(hotsheet-[^/]+)\/src\//.exec(path ?? '')?.[1];
  if (crate) {
    const output = command(repo, 'cargo', ['metadata', '--no-deps', '--format-version', '1']);
    if (!output) return broad('Cargo metadata is unavailable; keep the existing affected gate.');
    try {
      rustConsumers = rustConsumersFromMetadata(JSON.parse(output), crate);
    } catch {
      rustConsumers = null;
    }
    if (!rustConsumers) return broad('Cargo dependency graph is unknown or ambiguous.');
  }
  if (path === RUST_PRIVATE_PATH) {
    if (!base) return broad('Git baseline unavailable for the private Rust diff.', rustConsumers);
    const rustBefore = command(repo, 'git', ['show', `${base}:${path}`]);
    let rustAfter;
    try {
      rustAfter = readFileSync(join(repo, path), 'utf8');
    } catch {
      return broad('Current private Rust source is unreadable.', rustConsumers);
    }
    if (!existsSync(join(repo, 'crates/hotsheet-cli/tests/broken_pipe.rs')))
      return broad('The CLI broken-pipe integration test is missing.', rustConsumers);
    return edgePreview({ changes, rustBefore, rustAfter, rustConsumers });
  }
  if (path === WEB_PRIVATE_PATH) {
    const webBefore = base ? command(repo, 'git', ['show', `${base}:${path}`]) : null;
    if (webBefore === null) return broad('The private web module is new or its baseline is unavailable.');
    let webBoundaryReady = false;
    try {
      const webAfter = readFileSync(join(repo, path), 'utf8');
      const api = readFileSync(join(repo, WEB_PUBLIC_PATH), 'utf8');
      const lint = readFileSync(join(repo, 'clients/web/eslint.config.mjs'), 'utf8');
      const contract = readFileSync(join(repo, 'clients/web/src/trailing-task.test.ts'), 'utf8');
      const composition = readFileSync(join(repo, 'clients/web/tests/providers.spec.ts'), 'utf8');
      const runtime = readFileSync(join(repo, 'clients/web/src/app/runtime.tsx'), 'utf8');
      webBoundaryReady =
        webBefore !== webAfter &&
        api.includes("from './trailing-task-internal'") &&
        lint.includes("'**/trailing-task-internal'") &&
        contract.includes('HS2-K5E2KF preserves the public delay') &&
        composition.includes('defers ticket refresh without hiding an open metadata popup') &&
        runtime.includes("from '../trailing-task'") &&
        runtime.includes('createTrailingTask<TicketView>');
    } catch {
      // A missing proof is a broad plan.
    }
    return edgePreview({ changes, webBoundaryReady });
  }
  return edgePreview({ changes, rustConsumers });
}
