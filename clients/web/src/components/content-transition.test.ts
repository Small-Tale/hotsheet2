import { readFile } from 'node:fs/promises';

import { describe,expect,it } from 'vitest';

import { ContentTransition } from './content-transition';

describe('ContentTransition',()=>{
  it('keeps stable A and B sides while exposing only the active side',()=>{const markup=String(ContentTransition({active:'b',style:'push',direction:'forward',a:'First' as never,b:'Second' as never}));expect(markup).toContain('data-active-side="b"');expect(markup.match(/content-transition__side/g)).toHaveLength(2);expect(markup).toMatch(/data-side="a"[^>]*data-active="false"[^>]*aria-hidden="true"[^>]*inert/);expect(markup).toMatch(/data-side="b"[^>]*data-active="true"/)});
  it('supports backward push, crossfade, and motion-free replacement contracts',()=>{for(const style of ['push','crossfade','none'] as const)expect(String(ContentTransition({active:'a',style,direction:'backward',a:'A' as never,b:'B' as never}))).toContain(`data-transition-style="${style}"`)})
  it('pairs incoming and outgoing animations and honors reduced motion',async()=>{const css=await readFile(new URL('./content-transition.css',import.meta.url),'utf8');expect(css).toMatch(/data-active-side="b"[^}]+data-side="a"[^}]+push-out-start/);expect(css).toMatch(/data-active-side="b"[^}]+data-side="b"[^}]+push-in-end/);expect(css).toMatch(/data-active-side="a"[^}]+data-side="a"[^}]+push-in-start/);expect(css).toMatch(/data-active-side="a"[^}]+data-side="b"[^}]+push-out-end/);expect(css).toMatch(/prefers-reduced-motion:reduce[^}]+animation-duration:1ms/s)})
});
