import { describe, expect, it } from 'vitest';

import { specIndex } from '../../../scripts/browser-engines.mjs';
import {
  localWebkitUse,
  runnerWebkitGuard,
  WEBKIT_REPEAT_OVERRIDE,
  webkitRepeatGuard,
} from './playwright-webkit-guard.mjs';

const specs = {
  'tests/providers.spec.ts': { launched: [], optIn: [] },
  'tests/terminal-safari.spec.ts': { launched: ['webkit'], optIn: [] },
  'tests/webkit-memory.spec.ts': { launched: ['webkit'], optIn: ['HOTSHEET_WEBKIT_MEMORY'] },
};
const runner = (...args) => ['node', 'node_modules/@playwright/test/cli.js', 'test', ...args];
const guard = (argv, env = {}) => webkitRepeatGuard({ argv, env, specs });

describe('local WebKit repeat guard (HS2-CKHSM5)', () => {
  it('refuses --repeat-each when a selected spec launches WebKit, in both flag forms', () => {
    for (const argv of [
      runner('tests/terminal-safari.spec.ts', '--repeat-each=4'),
      runner('--repeat-each', '3', 'terminal-safari'),
      runner('--repeat-each=2'),
    ]) {
      const decision = guard(argv);
      expect(decision.refuse, argv.join(' ')).toContain('Refusing --repeat-each');
      expect(decision.refuse).toContain('tests/terminal-safari.spec.ts');
      expect(decision.refuse).toContain(WEBKIT_REPEAT_OVERRIDE);
      expect(decision.warn).toContain('no machine-wide browser slot pool');
    }
  });

  it('refuses --repeat-each with --browser webkit or all, whatever the files', () => {
    expect(guard(runner('tests/providers.spec.ts', '--browser=webkit', '--repeat-each=2')).refuse).toContain(
      '--browser webkit',
    );
    expect(guard(runner('providers', '--browser', 'all', '--repeat-each', '2')).refuse).toContain('--browser all');
  });

  it('allows Chromium-only repeats, single WebKit runs, the override, --list, CI and workers', () => {
    expect(guard(runner('tests/providers.spec.ts', '--repeat-each=20'))).toEqual({ refuse: null, warn: null });
    const single = guard(runner('tests/terminal-safari.spec.ts'));
    expect(single.refuse).toBeNull();
    expect(single.warn).toContain('terminal-safari');
    const overridden = guard(runner('terminal-safari', '--repeat-each=4'), { [WEBKIT_REPEAT_OVERRIDE]: '1' });
    expect(overridden.refuse).toBeNull();
    expect(overridden.warn).not.toBeNull();
    expect(guard(runner('--list', '--repeat-each=4'))).toEqual({ refuse: null, warn: null });
    expect(guard(runner('--repeat-each=4'), { CI: 'true' })).toEqual({ refuse: null, warn: null });
    expect(guard(runner('--repeat-each=4'), { TEST_WORKER_INDEX: '0' })).toEqual({ refuse: null, warn: null });
    expect(guard(['node', 'merge-reports'])).toEqual({ refuse: null, warn: null });
  });

  it('counts an opt-in WebKit spec only when its flag is set, and skips valued options', () => {
    expect(localWebkitUse({ args: ['webkit-memory'], env: {}, specs }).webkit).toEqual([]);
    expect(localWebkitUse({ args: ['webkit-memory'], env: { HOTSHEET_WEBKIT_MEMORY: '1' }, specs }).webkit).toEqual([
      'tests/webkit-memory.spec.ts',
    ]);
    // `--grep safari` is an option value, not a file filter.
    expect(localWebkitUse({ args: ['providers', '--grep', 'safari'], env: {}, specs }).webkit).toEqual([]);
  });

  it('finds the real spec-launched WebKit files in this package', () => {
    const real = specIndex(new URL('..', import.meta.url).pathname);
    const webkit = localWebkitUse({ args: [], env: {}, specs: real }).webkit;
    expect(webkit).toContain('tests/terminal-safari.spec.ts');
    expect(webkit).not.toContain('tests/providers.spec.ts');
    expect(webkitRepeatGuard({ argv: runner('--repeat-each=2'), env: {}, specs: real }).refuse).not.toBeNull();
    expect(runnerWebkitGuard(runner('tests/terminal-safari.spec.ts', '--repeat-each=2'), {}).refuse).toContain(
      'tests/terminal-safari.spec.ts',
    );
  });
});
