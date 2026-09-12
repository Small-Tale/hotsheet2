import { readFileSync } from 'node:fs';

import { describe,expect,it } from 'vitest';

import { TerminalTicketRail } from './terminal-ticket-rail';

describe('TerminalTicketRail',()=>{
  const props={projects:[{id:'one',name:'Project One'},{id:'two',name:'Project Two'}],selectedProjectId:'one',views:[{id:'all',label:'Queue'},{id:'backlog',label:'Backlog'}],selectedViewId:'all',controls:'Controls' as never,content:'Tickets' as never,inspector:'Inspector' as never};
  it('starts on a compact project ticket surface with intrinsic selectors and a hide action',()=>{const markup=String(TerminalTicketRail({...props,active:'root'}));expect(markup).toContain('name="terminal-rail-project"');expect(markup).toContain('Project One');expect(markup).toContain('name="terminal-rail-view"');expect(markup).toContain('Queue');expect(markup).toContain('aria-label="Hide ticket rail"');expect(markup).toContain('data-active-side="a"')});
  it('uses the shared backward pop when returning from a ticket',()=>{const markup=String(TerminalTicketRail({...props,active:'root',direction:'backward'}));expect(markup).toContain('data-active-side="a"');expect(markup).toContain('data-transition-style="push"');expect(markup).toContain('data-transition-direction="backward"')});
  it('keeps compact search last, animates active search onto a full row, and centers the ticket header independently',()=>{const css=readFileSync(new URL('./terminal-ticket-rail.css',import.meta.url),'utf8');expect(css).toMatch(/\.view-mode-switcher \{[^}]*border-radius:var\(--wa-border-radius-m\)[^}]*grid-column:1 \/ -1/);expect(css).toMatch(/workspace-header__utility-group \{[^}]*grid-column:2/);expect(css).toMatch(/workspace-header__search-group \{[^}]*grid-column:3[^}]*grid-row:2[^}]*transition:width \.25s ease/);expect(css).toMatch(/workspace-header__search-group\[data-expanded="true"\] \{[^}]*width:100%[^}]*grid-column:1 \/ -1[^}]*grid-row:3[^}]*animation:terminal-ticket-rail-search-enter \.25s ease/);expect(css).not.toMatch(/ticket-inspector__header > \.kui-toolbar \{[^}]*padding-left/);expect(css).toMatch(/ticket-inspector__header > \.kui-toolbar \{[^}]*grid-template-columns:1fr auto 1fr/);expect(css).toMatch(/terminal-ticket-rail__back \{[^}]*width:2\.25rem[^}]*color:var\(--wa-color-brand-on-quiet\)/)});
});
