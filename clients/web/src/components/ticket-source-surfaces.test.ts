import {readFileSync} from 'node:fs';

import {describe,expect,it} from 'vitest';

import {providerName,ProviderSetupForm} from './provider-setup-form';
import {TicketSourceSetupDialog} from './ticket-source-setup-dialog';
import {TicketSourcesSettings} from './ticket-sources-settings';

describe('ticket source surfaces',()=>{
  it('renders the setup root and preserves all five delegated actions',()=>{
    const markup=String(TicketSourceSetupDialog({project:{root:'/work/demo',name:'Demo',stores:[],needsTicketSetup:true},providerConnections:[],navigation:'none'}));
    expect(markup).toContain('data-component="ticket-source-setup-dialog"');
    expect(markup).toContain('Demo</strong> is open');
    for(const action of ['create-project-git-source','create-project-git-source-custom','select-provider-kind'])expect(markup).toContain(`data-action="${action}"`);
    for(const label of ['Create a Hot Sheet 2 git ticket repository','Connect GitHub Issues','Connect GitLab Issues','Connect Jira Cloud'])expect(markup).toContain(label);
  });

  it('renders provider editing, GitHub authorization, and remote-backup states from props',()=>{
    const auth=String(ProviderSetupForm({kind:'github',auth:{session:'session',userCode:'ABCD',verificationUri:'https://github.com/login/device',state:'waiting'},error:'Try again'}));
    expect(auth).toContain('Waiting for GitHub…');expect(auth).toContain('ABCD');expect(auth).toContain('Try again');
    const remote=String(TicketSourceSetupDialog({project:{root:'/work/demo',name:'Demo',stores:['/work/demo.hs2']},providerConnections:[],navigation:'push',createdGitTicketStore:'/work/demo.hs2',remoteBusy:true,remoteError:'Remote failed'}));
    expect(remote).toContain('Back up this ticket repository');expect(remote).toContain('Connecting…');expect(remote).toContain('Remote failed');
    expect(providerName('jira')).toBe('Jira Cloud');
  });

  it('renders connected git and external sources with default metadata',()=>{
    const markup=String(TicketSourcesSettings({stores:['/work/demo.hs2'],providerConnections:[{id:'github-main',provider:'github',locator:'small-tale/hotsheet2',name:'Issues',default:true,settings:{}}]}));
    expect(markup).toContain('data-component="ticket-sources-settings"');expect(markup).toContain('1 git ticket source and 1 external provider');expect(markup).toContain('/work/demo.hs2');expect(markup).toContain('GitHub Issues');expect(markup).toContain('Default');
  });

  it('owns source and provider styles outside the global stylesheet',()=>{
    const global=readFileSync(new URL('../style.css',import.meta.url),'utf8');
    for(const selector of ['data-ticket-source-setup-dialog','.ticket-source-setup','.provider-setup-form','.ticket-provider-settings'])expect(global).not.toContain(selector);
    expect(readFileSync(new URL('./ticket-source-setup-dialog.css',import.meta.url),'utf8')).toContain('data-ticket-source-setup-dialog');
    expect(readFileSync(new URL('./provider-setup-form.css',import.meta.url),'utf8')).toContain('.provider-setup-form');
    expect(readFileSync(new URL('./ticket-sources-settings.css',import.meta.url),'utf8')).toContain('.ticket-provider-settings');
  });
});
