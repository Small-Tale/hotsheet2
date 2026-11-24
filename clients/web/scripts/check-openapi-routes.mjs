import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

import YAML from 'yaml';

import { routeMethods } from './sync-openapi-routes.mjs';

const server = new URL('../../../crates/hotsheet-server/src/lib.rs', import.meta.url);
const contract = new URL('../../../docs/contracts/v1/openapi.yaml', import.meta.url);

/** HS2-140YMZ: keep the cheap route inventory ahead of broad CI First suites. */
export function routeCoverageErrors(source, spec) {
  const errors = [];
  const routes = routeMethods(source);
  const paths = spec?.paths;
  if (!paths || typeof paths !== 'object' || Array.isArray(paths)) return ['OpenAPI paths must be a mapping'];

  for (const [path, methods] of routes) {
    const operations = paths[path];
    if (!operations || typeof operations !== 'object') {
      errors.push(`Undocumented route: ${path}`);
      continue;
    }
    for (const method of Object.keys(methods)) {
      const operation = operations[method];
      if (!operation || typeof operation !== 'object') {
        errors.push(`Undocumented method: ${method.toUpperCase()} ${path}`);
        continue;
      }
      if (!operation.operationId || !operation.responses || operation['x-hotsheet-contract-depth'] === 'route-only')
        errors.push(`Incomplete OpenAPI operation: ${method.toUpperCase()} ${path}`);
    }
  }
  for (const path of Object.keys(paths))
    if (!routes.has(path)) errors.push(`OpenAPI path has no server route: ${path}`);
  return errors;
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  try {
    const errors = routeCoverageErrors(readFileSync(server, 'utf8'), YAML.parse(readFileSync(contract, 'utf8')));
    if (errors.length) {
      for (const error of errors) console.error(`HS2-140YMZ ${error}`);
      process.exitCode = 1;
    } else console.log('HS2-140YMZ server routes have concrete OpenAPI operations');
  } catch (error) {
    console.error(`HS2-140YMZ route/OpenAPI check failed: ${error.message}`);
    process.exitCode = 1;
  }
}
