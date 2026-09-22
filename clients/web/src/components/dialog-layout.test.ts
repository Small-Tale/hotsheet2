import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

import { ValueTable } from '@kerfjs/ui/value-table';
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
    expect(markup).toContain('>Server build details<');
    expect(markup).toContain('<p class="app-heading__summary" id="connection-details-summary">Current state</p>');
    expect(markup).toContain('app-heading__icon');
    expect(markup).not.toContain('role="heading"');
    const table = String(
      ValueTable({ label: 'Build metadata', children: '<div><dt>Version</dt><dd>1</dd></div>' as never }),
    );
    expect(table).toContain('data-component="value-table"');
    expect(table).toContain('aria-label="Build metadata"');
    const css = readFileSync(resolve(import.meta.dirname, 'heading.css'), 'utf8');
    expect(css).not.toContain('border-bottom');
    expect(css).not.toContain('.kui-toolbar {');
    const commandCss = readFileSync(resolve(import.meta.dirname, 'command-settings-editor.css'), 'utf8');
    expect(commandCss).toContain('button:not(:where(.kui-toolbar-control-group > button))');
  });
});
