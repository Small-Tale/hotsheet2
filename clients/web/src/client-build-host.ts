import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

import { clientBuildIdFromHtml } from './client-build';

/**
 * The client build a production host serves (HS2-KPJWBK): read once from the `index.html` of the
 * `dist/` it pinned at launch, since a newer bundle is only served after a host restart. A missing or
 * unstamped document reports no id, so the host announces nothing.
 */
export function servedClientBuildId(distRoot: string): string | undefined {
  try {
    return clientBuildIdFromHtml(readFileSync(resolve(distRoot, 'index.html'), 'utf8'));
  } catch {
    return undefined;
  }
}
