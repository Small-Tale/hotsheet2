import { mountStaticTerminalViewport } from '../terminal-viewport';

// Fill all 80 cells explicitly: xterm's erase-to-end uses the default cell attributes,
// so CSI K leaves the right side black even while reverse video is active. The following
// absolute cursor move cancels delayed wrapping for every row except the final one.
const bar=(value:string)=>`\u001b[7m${value.padEnd(80).slice(0,80)}\u001b[0m`;
const line=(row:number,value:string)=>`\u001b[${row};1H${value}`;

const nanoSource = [
  "import { mount } from 'kerfjs';",
  '',
  "import { createApi } from './api';",
  "import { App } from './components/app';",
  '',
  'const root = document.querySelector(\'#app\');',
  '',
  'if (!(root instanceof HTMLElement)) {',
  "  throw new Error('Missing application root.');",
  '}',
  '',
  'const api = createApi(window.location);',
  'const app = mount(root, App, { api });',
  '',
  "window.addEventListener('pagehide', () => {",
  '  app.destroy();',
  '});',
  '',
  'export { app };',
];

export function terminalDemoOutput(id:string):string {
  if(id==='tests')return `\u001b[2J\u001b[H\u001b[1;32m PASS \u001b[0m src/components/terminal-dashboard.test.ts\r\n\r\n PASS renders an exact 80 x 24 terminal viewport\r\n PASS keeps terminal chrome outside the PTY aspect ratio\r\n PASS opens shared terminal actions from the footer\r\n\r\n Test Files  12 passed (12)\r\n      Tests  84 passed (84)\r\n   Duration  1.42s\r\n\r\nWatching for file changes…`;
  const source=nanoSource.map((value,index)=>line(index+3,`${String(index+1).padStart(3)}  ${value}`)).join('');
  return `\u001b[2J\u001b[H${bar('  GNU nano 8.4                       src/main.tsx')}${line(2,bar('File: src/main.tsx'))}${source}${line(22,bar('[ Read 19 lines ]                         80 columns x 24 rows'))}${line(23,bar('^G Help  ^O Write Out  ^W Where Is  ^K Cut  ^T Execute'))}${line(24,bar('^X Exit  ^R Read File  ^\\ Replace  ^U Paste  ^J Justify'))}`;
}

export function syncTerminalDemoViewports(root:HTMLElement,mounts:Map<HTMLElement,()=>void>):void {
  const elements=new Set(root.querySelectorAll<HTMLElement>('[data-component="terminal-viewport"][data-grid-policy="dashboard-80x24"]'));
  for(const[element,dispose]of mounts)if(!elements.has(element)){dispose();mounts.delete(element)}
  for(const element of elements){if(mounts.has(element))continue;mounts.set(element,mountStaticTerminalViewport(element,{output:terminalDemoOutput(element.dataset.terminalId??'shell'),autoFocus:element.dataset.displayMode==='interactive'}))}
}
