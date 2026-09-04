export interface TerminalSizeMessage {pty_size:{cols:number;rows:number};driven_by?:string|null}

export function terminalResizeClaim(viewerId:string,cols:number,rows:number,focus:boolean,visible:boolean):string {
  return JSON.stringify({resize:{viewer_id:viewerId,cols:Math.max(1,Math.floor(cols)),rows:Math.max(1,Math.floor(rows)),focus,visible}});
}

export function parseTerminalSizeMessage(value:string):TerminalSizeMessage|undefined {
  try{const parsed=JSON.parse(value) as {pty_size?:{cols?:unknown;rows?:unknown};driven_by?:string|null},cols=parsed.pty_size?.cols,rows=parsed.pty_size?.rows;if(Number.isInteger(cols)&&Number.isInteger(rows)&&(cols as number)>0&&(rows as number)>0)return parsed as TerminalSizeMessage}catch{/* terminal text input is not a control frame */}return undefined;
}

export const terminalReconnectDelay=(attempt:number)=>Math.min(8_000,250*(2**Math.max(0,attempt)));
export const TERMINAL_VIEWPORT_MIN_SCALE=.7;
export function terminalViewportScale(viewportCols:number,viewportRows:number,ptyCols:number,ptyRows:number):number {
  return Math.max(TERMINAL_VIEWPORT_MIN_SCALE,Math.min(1,viewportCols/ptyCols,viewportRows/ptyRows));
}

export function terminalBrowserWebSocketUrl(apiPath:string,terminalId:string,locationValue:Pick<Location,'protocol'|'host'>=location):string {
  const protocol=locationValue.protocol==='https:'?'wss:':'ws:';
  return `${protocol}//${locationValue.host}${apiPath}/terminals/${encodeURIComponent(terminalId)}/attach`;
}

export function mountTerminalViewport(element:HTMLElement,{url,viewerId=crypto.randomUUID()}:{url:string;viewerId?:string}):()=>void {
  let disposed=false,disposeRuntime:(()=>void)|undefined;
  element.dataset.connection='loading';
  void import('./terminal-viewport-runtime').then(({mountTerminalViewportRuntime})=>{if(disposed)return;disposeRuntime=mountTerminalViewportRuntime(element,{url,viewerId})});
  return ()=>{disposed=true;disposeRuntime?.()};
}
