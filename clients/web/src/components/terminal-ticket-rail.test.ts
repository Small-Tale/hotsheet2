import { describe,expect,it } from 'vitest';

import { TerminalTicketRail } from './terminal-ticket-rail';

describe('TerminalTicketRail',()=>{
  const props={projects:[{id:'one',name:'Project One'},{id:'two',name:'Project Two'}],selectedProjectId:'one',controls:'Controls' as never,content:'Tickets' as never,inspector:'Inspector' as never};
  it('starts on a compact project ticket surface',()=>{const markup=String(TerminalTicketRail({...props,active:'root'}));expect(markup).toContain('name="terminal-rail-project"');expect(markup).toContain('Project One');expect(markup).toContain('Queue');expect(markup).toContain('data-active-side="a"')});
  it('uses the shared backward pop when returning from a ticket',()=>{const markup=String(TerminalTicketRail({...props,active:'root',direction:'backward'}));expect(markup).toContain('data-active-side="a"');expect(markup).toContain('data-transition-style="push"');expect(markup).toContain('data-transition-direction="backward"')});
});
