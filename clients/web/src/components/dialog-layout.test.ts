import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

import { ValueTable, ValueTableRow } from '@kerfjs/ui/value-table';
import { describe, expect, it } from 'vitest';

import { ConnectionDetailsDialog } from './connection-details-dialog';

// Exercise the shipped composition, including the ids that name its native dialog.
describe('dialog layout primitives', () => {
  it('composes accessible title and supporting copy around a divider-free Toolbar', () => {
    const markup = String(
      ConnectionDetailsDialog({
        assessment: {
          kind: 'unknown',
          detail: 'Current state',
          canRestartServer: false,
          revisionMismatch: false,
          sourceStale: false,
        },
      }),
    );
    expect(markup).toContain('aria-labelledby="connection-details-title"');
    expect(markup).toContain('aria-describedby="connection-details-summary"');
    expect(markup).toContain('data-component="toolbar"');
    expect(markup).not.toContain('divider-sides=');
    expect(markup).toContain('id="connection-details-title"');
    expect(markup).toContain('data-size="xlarge"');
    expect(markup).toContain('>Server Build Details<');
    // Kerf ToolbarText carries the supporting line (KF-DXVMBN, HS2-XHC39W).
    expect(markup).toMatch(/id="connection-details-summary"[^>]*>Current state</);
    expect(markup).not.toContain('app-heading');
    expect(markup).toContain('data-tile-tone="brand"');
    expect(markup).not.toContain('role="heading"');
    const table = String(
      ValueTable({ label: 'Build metadata', children: ValueTableRow({ label: 'Version', value: '1' }) }),
    );
    expect(table).toContain('data-component="value-table"');
    expect(table).toContain('aria-label="Build metadata"');
    expect(table).toContain('class="kui-value-table__row"');
    const commandCss = readFileSync(resolve(import.meta.dirname, 'command-settings-editor.css'), 'utf8');
    // The editor styles no buttons at all: never a descendant `button` such as the Done control inside the
    // Kerf ToolbarControlGroup or a nested component's buttons (HS2-JSSMFY), and its group delete is now a
    // Kerf ListHeader action (HS2-7BBRJT).
    expect(commandCss).not.toContain('.command-settings-editor__button');
    expect(commandCss).not.toMatch(/\.command-settings-editor\s+button\b/);
  });
});
