import { mountStaticTerminalViewport } from '../terminal-viewport';

const bar=(value:string)=>`\u001b[7m${value.padEnd(80).slice(0,80)}\u001b[0m`;

export function terminalDemoOutput(id:string):string {
  if(id==='tests')return `\u001b[2J\u001b[H\u001b[1;32m PASS \u001b[0m src/components/terminal-dashboard.test.ts\r\n\r\n PASS renders an exact 80 x 24 terminal viewport\r\n PASS keeps terminal chrome outside the PTY aspect ratio\r\n PASS opens shared terminal actions from the footer\r\n\r\n Test Files  12 passed (12)\r\n      Tests  84 passed (84)\r\n   Duration  1.42s\r\n\r\nWatching for file changes…`;
  return `\u001b[2J\u001b[H${bar('  GNU nano 8.4                       src/main.tsx')}\u001b[2;1H${bar('File: src/main.tsx')}\u001b[4;5Himport { mount } from 'kerfjs';\u001b[6;5Hconst app = mount(root, App);\u001b[12;23H80 columns x 24 rows\u001b[23;1H${bar('^G Help  ^O Write Out  ^W Where Is  ^K Cut  ^T Execute')}\u001b[24;1H${bar('^X Exit  ^R Read File  ^\\ Replace  ^U Paste  ^J Justify')}`;
}

export function syncTerminalDemoViewports(root:HTMLElement,mounts:Map<HTMLElement,()=>void>):void {
  const elements=new Set(root.querySelectorAll<HTMLElement>('[data-component="terminal-viewport"][data-grid-policy="dashboard-80x24"]'));
  for(const[element,dispose]of mounts)if(!elements.has(element)){dispose();mounts.delete(element)}
  for(const element of elements){if(mounts.has(element))continue;mounts.set(element,mountStaticTerminalViewport(element,{output:terminalDemoOutput(element.dataset.terminalId??'shell'),autoFocus:element.dataset.displayMode==='interactive'}))}
}
