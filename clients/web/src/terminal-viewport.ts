export interface TerminalSizeMessage {pty_size:{cols:number;rows:number};driven_by?:string|null}

export function terminalResizeClaim(viewerId:string,cols:number,rows:number,focus:boolean,visible:boolean):string {
  return JSON.stringify({resize:{viewer_id:viewerId,cols:Math.max(1,Math.floor(cols)),rows:Math.max(1,Math.floor(rows)),focus,visible}});
}

export function parseTerminalSizeMessage(value:string):TerminalSizeMessage|undefined {
  try{const parsed=JSON.parse(value) as {pty_size?:{cols?:unknown;rows?:unknown};driven_by?:string|null},cols=parsed.pty_size?.cols,rows=parsed.pty_size?.rows;if(Number.isInteger(cols)&&Number.isInteger(rows)&&(cols as number)>0&&(rows as number)>0)return parsed as TerminalSizeMessage}catch{/* terminal text input is not a control frame */}return undefined;
}

export const terminalReconnectDelay=(attempt:number)=>Math.min(8_000,250*(2**Math.max(0,attempt)));
export const TERMINAL_RESIZE_SETTLE_MS=120;
export const TERMINAL_PREVIEW_NATURAL_WIDTH=1280;
export const TERMINAL_PREVIEW_NATURAL_HEIGHT=960;
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

export function terminalShouldAdoptServerSize(scaledPreview:boolean,locallyFocused:boolean,drivenByViewer:boolean):boolean {
  return !scaledPreview&&!locallyFocused&&!drivenByViewer;
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
