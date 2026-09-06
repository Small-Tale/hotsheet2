import {describe,expect,it} from 'vitest';

import {defaultTerminalName,parseTerminalNames,terminalNameKey} from './terminal-names';

describe('terminal names',()=>{
  it('replaces generated terminal ids with a stable human sequence and tidies explicit ids',()=>{
    expect(defaultTerminalName('01JEDZC1ATK1BH4KGAZC3Z6W4D',0)).toBe('Terminal 1');
    expect(defaultTerminalName('codex-main',1)).toBe('Codex Main');
    expect(defaultTerminalName('CI_shell',2)).toBe('CI Shell');
  });
  it('loads only non-empty string overrides and scopes their keys to a project',()=>{
    expect(terminalNameKey('project','terminal')).toBe('project:terminal');
    expect(parseTerminalNames('{"one":"  Build shell  ","bad":2,"empty":" "}')).toEqual({one:'Build shell'});
    expect(parseTerminalNames('bad json')).toEqual({});
  });
});
