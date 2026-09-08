import {describe,expect,it} from 'vitest';

import {TicketEmptyState,ticketEmptyStateCopy} from './ticket-empty-state';

describe('TicketEmptyState',()=>{
  it('distinguishes a new project, an empty view, pending search, and no matches',()=>{
    expect(ticketEmptyStateCopy({kind:'project'}).title).toBe('No tickets yet');
    expect(ticketEmptyStateCopy({kind:'view',viewLabel:'Backlog'}).title).toBe('No tickets in Backlog');
    expect(ticketEmptyStateCopy({kind:'searching',query:'parser'})).toMatchObject({title:'Searching tickets',detail:'Looking for “parser”…'});
    expect(ticketEmptyStateCopy({kind:'search',query:'parser'})).toMatchObject({title:'No tickets match “parser”',detail:'Try a different search.'});
  });

  it('keeps compact board-column feedback concise',()=>{
    const markup=String(TicketEmptyState({kind:'view',viewLabel:'Started',compact:true}));
    expect(markup).toContain('data-component="ticket-empty-state"');expect(markup).toContain('data-compact="true"');expect(markup).toContain('No tickets in Started');expect(markup).not.toContain('Tickets will appear here');
  });
});
