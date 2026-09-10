export interface TerminalSizeMessage {pty_size:{cols:number;rows:number};driven_by?:string|null}

export function terminalResizeClaim(viewerId:string,cols:number,rows:number,focus:boolean,visible:boolean):string {
  const dimension=(value:number)=>Number.isFinite(value)?Math.max(1,Math.floor(value)):1;
  return JSON.stringify({resize:{viewer_id:viewerId,cols:dimension(cols),rows:dimension(rows),focus,visible}});
}

export function parseTerminalSizeMessage(value:string):TerminalSizeMessage|undefined {
  try{const parsed=JSON.parse(value) as {pty_size?:{cols?:unknown;rows?:unknown};driven_by?:string|null},cols=parsed.pty_size?.cols,rows=parsed.pty_size?.rows;if(Number.isInteger(cols)&&Number.isInteger(rows)&&(cols as number)>0&&(rows as number)>0)return parsed as TerminalSizeMessage}catch{/* terminal text input is not a control frame */}return undefined;
}

export const terminalReconnectDelay=(attempt:number)=>Math.min(8_000,250*(2**Math.max(0,attempt)));
export const TERMINAL_RESIZE_SETTLE_MS=120;
export const TERMINAL_DRAWER_RESIZE_END_EVENT='hotsheet-terminal-drawer-resize-end';
export const TERMINAL_DASHBOARD_COLS=80;
export const TERMINAL_DASHBOARD_ROWS=24;
export const TERMINAL_DASHBOARD_FONT_SIZE=24;
export const TERMINAL_DASHBOARD_LINE_HEIGHT=1.085;
export const TERMINAL_PREVIEW_NATURAL_WIDTH=1280;
export const TERMINAL_PREVIEW_NATURAL_HEIGHT=768;
export const TERMINAL_PREVIEW_SCROLLBACK=0;
export const TERMINAL_MAGNIFIED_SCROLLBACK=1_000;
export const TERMINAL_DEDICATED_SCROLLBACK=5_000;
// ANSI ESC is the external terminal protocol byte intentionally recognized here.
// eslint-disable-next-line no-control-regex
const ZSH_PROMPT_EOL_MARK=/^(?:\u001b\[(?:0|1)m)*\u001b\[7m%\u001b\[27m(?:\u001b\[(?:0|1|27)m)*(?:\r?\n)?/;

/** A late zsh attachment can begin at its reverse-video partial-line marker. The marker
 * described output that preceded the bounded replay, so presenting it as a new first row
 * is a reconstruction artifact rather than terminal content. */
export function stripLeadingZshPromptEolMark(value:Uint8Array<ArrayBuffer>):Uint8Array<ArrayBuffer>{
  const text=new TextDecoder().decode(value),trimmed=text.replace(ZSH_PROMPT_EOL_MARK,'');
  return trimmed===text?value:new TextEncoder().encode(trimmed);
}
export function terminalScrollbackLimit(displayMode:string|undefined,fixedDashboardGrid:boolean):number {
  if(displayMode==='scaled-preview')return TERMINAL_PREVIEW_SCROLLBACK;
  return fixedDashboardGrid?TERMINAL_MAGNIFIED_SCROLLBACK:TERMINAL_DEDICATED_SCROLLBACK;
}
export interface TerminalFocusRequest {projectId:string;terminalId:string}
export function terminalViewportShouldAutoFocus(request:TerminalFocusRequest|undefined,projectId:string,terminalId:string):boolean {
  return request?.projectId===projectId&&request.terminalId===terminalId;
}
export function terminalViewportScale(viewportCols:number,viewportRows:number,ptyCols:number,ptyRows:number):number {
  return Math.max(0,Math.min(1,viewportCols/ptyCols,viewportRows/ptyRows));
}
export function terminalPreviewScale(frameWidth:number,frameHeight:number):number {
  return Math.max(0,Math.min(frameWidth/TERMINAL_PREVIEW_NATURAL_WIDTH,frameHeight/TERMINAL_PREVIEW_NATURAL_HEIGHT));
}

export function terminalPhysicalScale(screenWidth:number,screenHeight:number,targetWidth:number,targetHeight:number):number {
  if([screenWidth,screenHeight,targetWidth,targetHeight].some(value=>!Number.isFinite(value)||value<=0))return 0;
  return Math.min(targetWidth/screenWidth,targetHeight/screenHeight);
}

export function terminalDedicatedGridSize(cols:number,rows:number) {
  return {cols:Math.max(1,cols),rows:Math.max(1,rows-1)};
}

export function terminalShouldAdoptServerSize(scaledPreview:boolean,locallyFocused:boolean,drivenByViewer:boolean):boolean {
  return !scaledPreview&&!locallyFocused&&!drivenByViewer;
}

export function terminalViewportClaimsSizingFocus(scaledPreview:boolean,fixedDashboardGrid:boolean,focusRequested:boolean,containsActiveElement:boolean):boolean {
  return fixedDashboardGrid||(!scaledPreview&&(focusRequested||containsActiveElement));
}

export function terminalBrowserWebSocketUrl(apiPath:string,terminalId:string,locationValue:Pick<Location,'protocol'|'host'>=location):string {
  const protocol=locationValue.protocol==='https:'?'wss:':'ws:';
  return `${protocol}//${locationValue.host}${apiPath}/terminals/${encodeURIComponent(terminalId)}/attach`;
}

export function mountTerminalViewport(element:HTMLElement,{url,viewerId=crypto.randomUUID(),autoFocus=false}:{url:string;viewerId?:string;autoFocus?:boolean}):()=>void {
  let disposed=false,disposeRuntime:(()=>void)|undefined;
  element.dataset.connection='loading';
  void import('./terminal-viewport-runtime').then(({mountTerminalViewportRuntime})=>{if(disposed)return;disposeRuntime=mountTerminalViewportRuntime(element,{url,viewerId,autoFocus})});
  return ()=>{disposed=true;disposeRuntime?.()};
}

export function mountStaticTerminalViewport(element:HTMLElement,{output,autoFocus=false}:{output:string;autoFocus?:boolean}):()=>void {
  let disposed=false,disposeRuntime:(()=>void)|undefined;
  element.dataset.connection='loading';
  void import('./terminal-viewport-runtime').then(({mountStaticTerminalViewportRuntime})=>{if(disposed)return;disposeRuntime=mountStaticTerminalViewportRuntime(element,{output,autoFocus})});
  return ()=>{disposed=true;disposeRuntime?.()};
}
