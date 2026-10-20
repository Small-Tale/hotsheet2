import { readFileSync, writeFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

import prettier from 'prettier';
import YAML from 'yaml';

const server = fileURLToPath(new URL('../../../crates/hotsheet-server/src/lib.rs', import.meta.url));
const contract = fileURLToPath(new URL('../../../docs/contracts/v1/openapi.yaml', import.meta.url));

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

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const source = readFileSync(server, 'utf8');
  const spec = YAML.parse(readFileSync(contract, 'utf8'));
  for (const [path, methods] of routeMethods(source)) {
    const operations = (spec.paths[path] ??= {});
    const parameters = [...path.matchAll(/\{([^}]+)\}/g)].map(([, name]) => name);
    if (parameters.length && !operations.parameters) {
      operations.parameters = parameters.map((name) => ({
        name,
        in: 'path',
        required: true,
        schema: { type: 'string' },
      }));
    }
    for (const [method, handler] of Object.entries(methods)) {
      operations[method] ??= {
        operationId: handler,
        'x-hotsheet-contract-depth': 'route-only',
        responses: {
          default: { description: 'Response shape still follows the server handler and wire DTO.' },
        },
      };
    }
  }
  const config = (await prettier.resolveConfig(contract)) ?? {};
  writeFileSync(
    contract,
    await prettier.format(YAML.stringify(spec, { lineWidth: 120 }), { ...config, filepath: contract }),
  );
}
