export interface TerminalSizeMessage {pty_size:{cols:number;rows:number};driven_by?:string|null}

export function terminalResizeClaim(viewerId:string,cols:number,rows:number,focus:boolean,visible:boolean):string {
  return JSON.stringify({resize:{viewer_id:viewerId,cols:Math.max(1,Math.floor(cols)),rows:Math.max(1,Math.floor(rows)),focus,visible}});
}

export function parseTerminalSizeMessage(value:string):TerminalSizeMessage|undefined {
  try{const parsed=JSON.parse(value) as {pty_size?:{cols?:unknown;rows?:unknown};driven_by?:string|null},cols=parsed.pty_size?.cols,rows=parsed.pty_size?.rows;if(Number.isInteger(cols)&&Number.isInteger(rows)&&(cols as number)>0&&(rows as number)>0)return parsed as TerminalSizeMessage}catch{/* terminal text input is not a control frame */}return undefined;
}

export const terminalReconnectDelay=(attempt:number)=>Math.min(8_000,250*(2**Math.max(0,attempt)));
export const TERMINAL_RESIZE_SETTLE_MS=120;
export const TERMINAL_DRAWER_RESIZE_END_EVENT='hotsheet-terminal-drawer-resize-end';
export const TERMINAL_DASHBOARD_COLS=80;
export const TERMINAL_DASHBOARD_ROWS=24;
export const TERMINAL_PREVIEW_NATURAL_WIDTH=1280;
export const TERMINAL_PREVIEW_NATURAL_HEIGHT=768;
export interface TerminalDashboardTypography {fontSize:number;letterSpacing:number;lineHeight:number}
export function terminalDashboardTypography(current:TerminalDashboardTypography,screenWidth:number,screenHeight:number,targetWidth:number,targetHeight:number):TerminalDashboardTypography {
  if([screenWidth,screenHeight,targetWidth,targetHeight].some(value=>!Number.isFinite(value)||value<=0))return current;
  const scale=Math.min(targetWidth/screenWidth,targetHeight/screenHeight),fontSize=Math.max(4,Math.min(72,current.fontSize*scale)),fontScale=fontSize/current.fontSize;
  const scaledWidth=screenWidth*fontScale,scaledHeight=screenHeight*fontScale;
  return {
    fontSize:Math.round(fontSize*100)/100,
    letterSpacing:Math.round(Math.max(0,current.letterSpacing+(targetWidth-scaledWidth)/TERMINAL_DASHBOARD_COLS)*100)/100,
    lineHeight:Math.round(Math.max(1,Math.min(5,current.lineHeight*targetHeight/scaledHeight))*1000)/1000,
  };
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
