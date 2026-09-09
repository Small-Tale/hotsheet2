import {describe,expect,it} from 'vitest';

import {ticketEmptyStateCopy} from './ticket-empty-state';

describe('TicketEmptyState',()=>{
  it('distinguishes loading, a new project, an empty view, pending search, and no matches',()=>{
    expect(ticketEmptyStateCopy({kind:'loading'})).toMatchObject({title:'Loading tickets',detail:'Opening this project…'});
    expect(ticketEmptyStateCopy({kind:'project'}).title).toBe('No tickets yet');
    expect(ticketEmptyStateCopy({kind:'view',viewLabel:'Backlog'}).title).toBe('No tickets in Backlog');
    expect(ticketEmptyStateCopy({kind:'searching',query:'parser'})).toMatchObject({title:'Searching tickets',detail:'Looking for “parser”…'});
    expect(ticketEmptyStateCopy({kind:'search',query:'parser'})).toMatchObject({title:'No tickets match “parser”',detail:'Try a different search.'});
  });
});
