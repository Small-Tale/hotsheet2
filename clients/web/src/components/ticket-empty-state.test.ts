import {describe,expect,it} from 'vitest';

import {TicketEmptyState,ticketEmptyStateCopy} from './ticket-empty-state';

describe('TicketEmptyState',()=>{
  it('distinguishes loading, a new project, an empty view, pending search, and no matches',()=>{
    expect(ticketEmptyStateCopy({kind:'loading'})).toMatchObject({title:'Loading tickets',detail:'Opening this project…'});
    expect(ticketEmptyStateCopy({kind:'project'}).title).toBe('No tickets yet');
    expect(ticketEmptyStateCopy({kind:'view',viewLabel:'Backlog'}).title).toBe('No tickets in Backlog');
    expect(ticketEmptyStateCopy({kind:'searching',query:'parser'})).toMatchObject({title:'Searching tickets',detail:'Looking for “parser”…'});
    expect(ticketEmptyStateCopy({kind:'search',query:'parser'})).toMatchObject({title:'No tickets match “parser”',detail:'Try a different search.'});
  });

  it('maps product copy and loading state into the shared Kerf empty-state primitive',()=>{
    const loading=String(TicketEmptyState({kind:'loading'}));
    expect(loading).toContain('data-component="empty-state"');
    expect(loading).toContain('class="kui-empty-state ticket-empty-state ticket-empty-state--loading"');
    expect(loading).toContain('aria-busy="true"');
    expect(loading).toContain('data-component="loading-spinner"');
    expect(String(TicketEmptyState({kind:'project'}))).toContain('aria-busy="false"');
  });
});
