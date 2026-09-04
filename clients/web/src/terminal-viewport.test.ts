import { describe,expect,it } from 'vitest';

import { parseTerminalSizeMessage,terminalBrowserWebSocketUrl,terminalReconnectDelay,terminalResizeClaim,terminalViewportScale } from './terminal-viewport';

describe('terminal viewport protocol',()=>{
  it('builds a credential-free same-origin attach URL',()=> { expect(terminalBrowserWebSocketUrl('/__hotsheet/project-api/project%20one','codex/main',{protocol:'https:',host:'hs.test'})).toBe('wss://hs.test/__hotsheet/project-api/project%20one/terminals/codex%2Fmain/attach'); });
  it('encodes leased viewport identity, focus, visibility, and bounded dimensions',()=> { expect(JSON.parse(terminalResizeClaim('viewer-1',0,24.9,true,false))).toEqual({resize:{viewer_id:'viewer-1',cols:1,rows:24,focus:true,visible:false}}); });
  it('accepts only valid server size broadcasts',()=>{expect(parseTerminalSizeMessage('{"pty_size":{"cols":120,"rows":40},"driven_by":"viewer-1"}')).toEqual({pty_size:{cols:120,rows:40},driven_by:'viewer-1'});expect(parseTerminalSizeMessage('terminal text')).toBeUndefined();expect(parseTerminalSizeMessage('{"pty_size":{"cols":0,"rows":40}}')).toBeUndefined()});
  it('caps exponential reconnect backoff',()=> { expect([0,1,2,8].map(terminalReconnectDelay)).toEqual([250,500,1000,8000]); });
  it('letterboxes smaller PTYs, scales larger PTYs, and preserves a readable floor',()=>{expect(terminalViewportScale(120,40,80,24)).toBe(1);expect(terminalViewportScale(80,24,100,30)).toBe(.8);expect(terminalViewportScale(40,12,120,40)).toBe(.7)});
});
