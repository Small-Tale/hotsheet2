import { describe,expect,it } from 'vitest';

import { parseTerminalSizeMessage,stripLeadingZshPromptEolMark,TERMINAL_DASHBOARD_COLS,TERMINAL_DASHBOARD_FONT_SIZE,TERMINAL_DASHBOARD_LINE_HEIGHT,TERMINAL_DASHBOARD_ROWS,TERMINAL_DEDICATED_SCROLLBACK,TERMINAL_DRAWER_RESIZE_END_EVENT,TERMINAL_MAGNIFIED_SCROLLBACK,TERMINAL_PREVIEW_NATURAL_HEIGHT,TERMINAL_PREVIEW_NATURAL_WIDTH,TERMINAL_PREVIEW_SCROLLBACK,TERMINAL_RESIZE_SETTLE_MS,terminalBrowserWebSocketUrl,terminalDedicatedGridSize,terminalFittedFontSize,terminalPhysicalScale,terminalPreviewScale,terminalReconnectDelay,terminalResizeClaim,terminalScrollbackLimit,terminalShouldAdoptServerSize,terminalShouldUseWebgl,terminalUsesMobile80xM,terminalViewportClaimsSizingFocus,terminalViewportScale,terminalViewportShouldAutoFocus } from './terminal-viewport';

describe('terminal viewport protocol',()=>{
  it('builds a credential-free same-origin attach URL',()=> { expect(terminalBrowserWebSocketUrl('/__hotsheet/project-api/project%20one','codex/main',{protocol:'https:',host:'hs.test'})).toBe('wss://hs.test/__hotsheet/project-api/project%20one/terminals/codex%2Fmain/attach'); });
  it('encodes leased viewport identity, focus, visibility, interaction, and bounded finite dimensions',()=> {
    // interacting defaults to false (a heartbeat/geometry claim) so it can't advance the arbiter's recency (HS2-3ZBQDG).
    expect(JSON.parse(terminalResizeClaim('viewer-1',0,24.9,true,false))).toEqual({resize:{viewer_id:'viewer-1',cols:1,rows:24,focus:true,visible:false,interacting:false}});
    expect(JSON.parse(terminalResizeClaim('viewer-2',Number.NaN,Number.POSITIVE_INFINITY,false,false))).toEqual({resize:{viewer_id:'viewer-2',cols:1,rows:1,focus:false,visible:false,interacting:false}});
    // A genuine interaction (tap/keystroke) marks the claim interacting so this device takes over sizing.
    expect(JSON.parse(terminalResizeClaim('viewer-3',100,40,true,true,true))).toEqual({resize:{viewer_id:'viewer-3',cols:100,rows:40,focus:true,visible:true,interacting:true}});
  });
  it('accepts only valid server size broadcasts',()=>{expect(parseTerminalSizeMessage('{"pty_size":{"cols":120,"rows":40},"driven_by":"viewer-1"}')).toEqual({pty_size:{cols:120,rows:40},driven_by:'viewer-1'});expect(parseTerminalSizeMessage('terminal text')).toBeUndefined();expect(parseTerminalSizeMessage('{"pty_size":{"cols":0,"rows":40}}')).toBeUndefined()});
  it('caps exponential reconnect backoff',()=> { expect([0,1,2,8].map(terminalReconnectDelay)).toEqual([250,500,1000,8000]); });
  it('sends a final resize claim shortly after a drag settles',()=>{expect(TERMINAL_RESIZE_SETTLE_MS).toBe(120)});
  it('uses one explicit drawer-resize completion event',()=>{expect(TERMINAL_DRAWER_RESIZE_END_EVENT).toBe('hotsheet-terminal-drawer-resize-end')});
  it('focuses only the exact newly created project terminal',()=>{const request={projectId:'project-a',terminalId:'new-terminal'};expect(terminalViewportShouldAutoFocus(request,'project-a','new-terminal')).toBe(true);expect(terminalViewportShouldAutoFocus(request,'project-a','old-terminal')).toBe(false);expect(terminalViewportShouldAutoFocus(request,'project-b','new-terminal')).toBe(false);expect(terminalViewportShouldAutoFocus(undefined,'project-a','new-terminal')).toBe(false)});
  it('letterboxes smaller PTYs and always contains larger PTYs without clipping',()=>{expect(terminalViewportScale(120,40,80,24)).toBe(1);expect(terminalViewportScale(80,24,100,30)).toBe(.8);expect(terminalViewportScale(40,12,120,40)).toBe(.3)});
  it('uniformly scales the fixed 80 by 24 dashboard surface without changing its aspect',()=>{expect([TERMINAL_PREVIEW_NATURAL_WIDTH,TERMINAL_PREVIEW_NATURAL_HEIGHT]).toEqual([1280,768]);expect(terminalPreviewScale(640,384)).toBe(.5);expect(terminalPreviewScale(640,240)).toBe(.3125);expect(terminalPreviewScale(0,384)).toBe(0);expect(terminalPhysicalScale(1281,767,1280,768)).toBeCloseTo(1280/1281);expect(terminalPhysicalScale(0,767,1280,768)).toBe(0)});
  it('iteratively fits an interactive fixed grid by its measured physical scale',()=>{expect(terminalFittedFontSize(13,1.05)).toBeCloseTo(13.65);expect(terminalFittedFontSize(8,.25)).toBe(4);expect(terminalFittedFontSize(Number.NaN,1)).toBe(4)});
  it('does not duplicate deep scrollback into read-only dashboard previews',()=>{expect([TERMINAL_PREVIEW_SCROLLBACK,TERMINAL_MAGNIFIED_SCROLLBACK,TERMINAL_DEDICATED_SCROLLBACK]).toEqual([0,1_000,5_000]);expect(terminalScrollbackLimit('scaled-preview',true)).toBe(0);expect(terminalScrollbackLimit('interactive',true)).toBe(1_000);expect(terminalScrollbackLimit('interactive',false)).toBe(5_000)});
  it('uses the DOM renderer on Apple WebKit where WebGL can paint a blank glyph layer',()=>{
    expect(terminalShouldUseWebgl('Mozilla/5.0 (iPhone; CPU iPhone OS 27_0 like Mac OS X) AppleWebKit/605.1.15 Mobile/15E148 Safari/604.1')).toBe(false);
    expect(terminalShouldUseWebgl('Mozilla/5.0 (iPhone; CPU iPhone OS 27_0 like Mac OS X) AppleWebKit/605.1.15 CriOS/150.0 Mobile/15E148 Safari/604.1')).toBe(false);
    expect(terminalShouldUseWebgl('Mozilla/5.0 (Macintosh) AppleWebKit/605.1.15 Version/27.0 Safari/605.1.15')).toBe(false);
    expect(terminalShouldUseWebgl('Mozilla/5.0 AppleWebKit/537.36 Chrome/150.0.0.0 Safari/537.36')).toBe(true);
    expect(terminalShouldUseWebgl('Mozilla/5.0 Gecko/20100101 Firefox/150.0')).toBe(true);
  });
  it('uses 80xM for mobile interactive surfaces but keeps preview tiles at 80x24',()=>{
    expect(terminalUsesMobile80xM(true,true,false,false)).toBe(true);
    expect(terminalUsesMobile80xM(true,false,false,true)).toBe(true);
    expect(terminalUsesMobile80xM(true,true,true,false)).toBe(false);
    expect(terminalUsesMobile80xM(false,true,false,false)).toBe(false);
    expect(terminalUsesMobile80xM(false,false,false,true)).toBe(false);
  });
  it('removes only a leading styled zsh partial-line marker from late fixed-grid replay',()=>{const encode=(value:string)=>new TextEncoder().encode(value),decode=(value:Uint8Array)=>new TextDecoder().decode(value);expect(decode(stripLeadingZshPromptEolMark(encode('\u001b[1m\u001b[7m%\u001b[27m\u001b[1m\u001b[0m\r\nprompt % ')))).toBe('prompt % ');expect(decode(stripLeadingZshPromptEolMark(encode('%\r\nprompt % ')))).toBe('%\r\nprompt % ');expect(decode(stripLeadingZshPromptEolMark(encode('\u001b[7m% intentional\u001b[0m')))).toBe('\u001b[7m% intentional\u001b[0m')});
  it('reserves one physical containment row in dedicated terminals',()=>{expect(terminalDedicatedGridSize(100,30)).toEqual({cols:100,rows:29});expect(terminalDedicatedGridSize(1,1)).toEqual({cols:1,rows:1})});
  it('keeps dashboard grids at one immutable 80 by 24 canonical geometry',()=>{expect([TERMINAL_DASHBOARD_COLS,TERMINAL_DASHBOARD_ROWS,TERMINAL_DASHBOARD_FONT_SIZE,TERMINAL_DASHBOARD_LINE_HEIGHT]).toEqual([80,24,24,1.085])});
  it('lets fixed 80 by 24 surfaces drive sizing without accepting keyboard input',()=>{expect(terminalViewportClaimsSizingFocus(true,true,false,false)).toBe(true);expect(terminalViewportClaimsSizingFocus(false,true,false,false)).toBe(true);expect(terminalViewportClaimsSizingFocus(false,false,false,false)).toBe(false);expect(terminalViewportClaimsSizingFocus(false,false,true,false)).toBe(true);expect(terminalViewportClaimsSizingFocus(false,false,false,true)).toBe(true)});
  it('keeps preview and locally authoritative grids stable when server-size messages race',()=>{expect(terminalShouldAdoptServerSize(true,false,false)).toBe(false);expect(terminalShouldAdoptServerSize(false,true,false)).toBe(false);expect(terminalShouldAdoptServerSize(false,false,true)).toBe(false);expect(terminalShouldAdoptServerSize(false,false,false)).toBe(true)});
});
