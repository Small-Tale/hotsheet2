import {readFileSync} from 'node:fs';

import {describe,expect,it} from 'vitest';

describe('Drive options lifecycle (HS2-S010QF)',()=>{
  const source=readFileSync(new URL('./main.tsx',import.meta.url),'utf8');

  it('keeps the owning menu open across provider, model, effort, default, and manual-model selections',()=>{
    for(const action of ['select-drive-default','select-drive-tool','select-drive-model','select-drive-effort']){
      const start=source.indexOf(`'[data-action="${action}"]'`),end=source.indexOf('\n',start);
      expect(start,action).toBeGreaterThan(0);
      expect(source.slice(start,end)).not.toContain('driveOptionsOpen.value=false');
    }
    const manualStart=source.indexOf("function openManualModel(target:"),manualEnd=source.indexOf('function restoreCommandEditorAfterManualModel',manualStart);
    expect(source.slice(manualStart,manualEnd)).not.toContain('driveOptionsOpen.value=false');
  });

  it('still closes from its toggle and a true outside pointer interaction',()=>{
    expect(source).toContain('driveOptionsOpen.value=opening');
    expect(source).toContain("event.composedPath().some(item=>item instanceof Element&&item.matches('.project-sidebar__drive-row'))");
  });
});
