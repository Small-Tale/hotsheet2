import {readFileSync} from 'node:fs';

import {describe,expect,it} from 'vitest';

const read=(path:string)=>readFileSync(new URL(path,import.meta.url),'utf8');

describe('global stylesheet ownership',()=>{
  it('keeps extracted surface selectors in their component stylesheets',()=>{
    const global=read('./style.css');
    const ownership=[
      ['project dialog','project-dialog.css',['data-project-dialog','.project-dialog','.remote-project-dialog']],
      ['ticket-source setup','ticket-source-setup-dialog.css',['data-ticket-source-setup-dialog','.ticket-source-setup']],
      ['provider setup','provider-setup-form.css',['.provider-setup-form']],
      ['ticket-source settings','ticket-sources-settings.css',['.ticket-provider-settings']],
      ['settings workspace','settings-workspace.css',['.project-settings']],
      ['terminal rename','terminal-rename-dialog.css',['.terminal-rename']],
      ['notification inspector','notification-inspector.css',['.notification-inspector-empty']],
    ] as const;
    for(const [name,file,selectors] of ownership){const owned=read(`./components/${file}`);for(const selector of selectors){expect(global,`${name} leaked ${selector} into style.css`).not.toContain(selector);expect(owned,`${name} does not own ${selector}`).toContain(selector)}}
  });

  it('keeps the intentionally global app-shell rules in style.css',()=>{
    const global=read('./style.css');
    for(const selector of [':root','html, body, #app','.app-shell[data-component="app-shell"]','.app-empty','.app-loading, .app-toast','.ticket-page-more'])expect(global).toContain(selector);
  });
});
