import { describe,expect,it } from 'vitest';

import { terminalDemoOutput } from './terminal-demo';

describe('terminal demo data',()=>{
  it('provides realistic ANSI terminal states for Nano and test sessions',()=>{
    const nano=terminalDemoOutput('shell'),tests=terminalDemoOutput('tests');
    expect(nano).toContain('\u001b[2J\u001b[H');expect(nano).toContain('GNU nano 8.4');expect(nano).toContain('80 columns x 24 rows');expect(nano).toContain('\u001b[21;1H');expect(nano).toContain('export { app };');expect(nano).toContain('\u001b[24;1H');expect(nano).toContain('^X Exit');expect(nano).not.toContain('\u001b[K');expect(nano).toContain(`${'^X Exit  ^R Read File  ^\\ Replace  ^U Paste  ^J Justify'.padEnd(80)}\u001b[0m`);
    expect(tests).toContain('\u001b[1;32m PASS ');expect(tests).toContain('84 passed');expect(tests).toContain('Watching for file changes');
  });
});
