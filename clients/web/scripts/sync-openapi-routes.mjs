import { readFileSync, writeFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

import prettier from 'prettier';
import YAML from 'yaml';

const server = fileURLToPath(new URL('../../../crates/hotsheet-server/src/lib.rs', import.meta.url));
const contract = fileURLToPath(new URL('../../../docs/contracts/v1/openapi.yaml', import.meta.url));
const readme = fileURLToPath(new URL('../../../docs/contracts/README.md', import.meta.url));

/** Read one balanced Axum `.route(...)` call without confusing a nested handler call with its end. */
function callAt(source, opening) {
  let depth = 0;
  let quoted = false;
  let escaped = false;
  for (let at = opening; at < source.length; at++) {
    const char = source[at];
    if (quoted) {
      if (escaped) escaped = false;
      else if (char === '\\') escaped = true;
      else if (char === '"') quoted = false;
    } else if (char === '"') quoted = true;
    else if (char === '(') depth++;
    else if (char === ')' && --depth === 0) return source.slice(opening + 1, at);
  }
  throw new Error('Unbalanced server route call');
}

export function routeMethods(source) {
  const paths = new Map();
  for (const match of source.matchAll(/\.route\(/g)) {
    const call = callAt(source, match.index + '.route'.length);
    const path = /^\s*"([^"]+)"\s*,/.exec(call)?.[1];
    if (!path) continue;
    const methods = [...call.matchAll(/(?:^|\W)(get|post|put|patch|delete)\(([\w:]+)\)/g)];
    if (methods.length === 0) throw new Error(`No HTTP method for ${path}`);
    paths.set(path, Object.fromEntries(methods.map(([, method, handler]) => [method, handler.split('::').at(-1)])));
  }
  return paths;
}

/** Count the contract's paths; docs/contracts/README.md states this number (HS2-ZVB932). */
export function contractPathCount(contractText) {
  return Object.keys(YAML.parse(contractText)?.paths ?? {}).length;
}

/** The route count docs/contracts/README.md claims, or null when its sentence is missing (HS2-ZVB932). */
export function readmeRouteCount(readmeText) {
  const match = /inventories all (\d+) current server router paths/.exec(readmeText.replace(/\s+/g, ' '));
  return match ? Number(match[1]) : null;
}

/**
 * HS2-ZVB932: add route-only stubs for new server routes and return the canonical contract text. Editing the
 * parsed Document keeps scalar styles, and `lineWidth: 0` stops yaml from folding, so Prettier (`proseWrap: always`
 * for docs/contracts in .prettierrc.json) is the only wrapper. A formatted, complete contract is a fixpoint.
 */
export async function syncContract(source, contractText, prettierOptions) {
  const document = YAML.parseDocument(contractText);
  if (document.errors.length) throw document.errors[0];
  for (const [path, methods] of routeMethods(source)) {
    if (!document.hasIn(['paths', path])) document.setIn(['paths', path], document.createNode({}));
    const parameters = [...path.matchAll(/\{([^}]+)\}/g)].map(([, name]) => name);
    if (parameters.length && !document.hasIn(['paths', path, 'parameters'])) {
      document.setIn(
        ['paths', path, 'parameters'],
        document.createNode(
          parameters.map((name) => ({ name, in: 'path', required: true, schema: { type: 'string' } })),
        ),
      );
    }
    for (const [method, handler] of Object.entries(methods)) {
      if (document.hasIn(['paths', path, method])) continue;
      document.setIn(
        ['paths', path, method],
        document.createNode({
          operationId: handler,
          'x-hotsheet-contract-depth': 'route-only',
          responses: {
            default: { description: 'Response shape still follows the server handler and wire DTO.' },
          },
        }),
      );
    }
  }
  return prettier.format(document.toString({ lineWidth: 0 }), prettierOptions);
}

/** Prettier options for the committed contract, including the repository's docs/contracts override. */
export async function contractPrettierOptions() {
  return { ...((await prettier.resolveConfig(contract)) ?? {}), filepath: contract };
}

/** Byte-check the committed contract and README against what `npm run contract:sync` would write (HS2-ZVB932). */
export async function contractSyncErrors(
  source = readFileSync(server, 'utf8'),
  contractText = readFileSync(contract, 'utf8'),
  readmeText = readFileSync(readme, 'utf8'),
) {
  const errors = [];
  if ((await syncContract(source, contractText, await contractPrettierOptions())) !== contractText)
    errors.push('docs/contracts/v1/openapi.yaml differs from `npm run contract:sync` output; run it from clients/web');
  const claimed = readmeRouteCount(readmeText);
  const actual = contractPathCount(contractText);
  if (claimed !== actual)
    errors.push(`docs/contracts/README.md states ${claimed ?? 'no'} router paths; the contract has ${actual}`);
  return errors;
}

if (process.argv[1] === fileURLToPath(import.meta.url) && process.argv.includes('--check')) {
  const errors = await contractSyncErrors();
  for (const error of errors) console.error(`HS2-ZVB932 ${error}`);
  if (errors.length) process.exitCode = 1;
} else if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const next = await syncContract(
    readFileSync(server, 'utf8'),
    readFileSync(contract, 'utf8'),
    await contractPrettierOptions(),
  );
  writeFileSync(contract, next);
  const count = contractPathCount(next);
  if (readmeRouteCount(readFileSync(readme, 'utf8')) !== count)
    console.warn(`HS2-ZVB932 update docs/contracts/README.md: the contract now has ${count} router paths`);
}
