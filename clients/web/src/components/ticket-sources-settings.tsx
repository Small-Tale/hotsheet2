import './ticket-sources-settings.css';

import {ListItem} from '@kerfjs/ui/list-item';
import {LucideIcon} from '@kerfjs/ui/lucide-icon';
import {Cable,ChevronRight} from 'lucide';

import type {ProviderConnection} from '../api';
import {type ExternalProviderKind,providerName} from './provider-setup-form';

export interface TicketSourcesSettingsProps{
  stores:readonly string[];
  providerConnections:readonly ProviderConnection[];
  error?:string;
  setupOpen?:boolean;
}

export function TicketSourcesSettings({stores,providerConnections,error='',setupOpen=false}:TicketSourcesSettingsProps){return <div class="ticket-provider-settings" data-component="ticket-sources-settings"><section><header class="ticket-provider-settings__header"><h2>Connected sources</h2><wa-button appearance="outlined" data-action="open-provider-dialog">Add data source</wa-button></header><p>This checkout uses {stores.length} git ticket source{stores.length===1?'':'s'} and {providerConnections.length} external provider{providerConnections.length===1?'':'s'}.</p>{stores.map(store=><div class="ticket-provider-settings__source"><strong>Hot Sheet git</strong><code>{store}</code></div>)}{providerConnections.length>0&&<div class="ticket-provider-settings__connections">{providerConnections.map(connection=><ListItem action="edit-provider-connection" itemId={connection.id} multiline accessibleLabel={`Edit ${connection.name??connection.id}`} icon={<LucideIcon icon={Cable} name="cable"/>} trailing={<LucideIcon icon={ChevronRight} name="chevron-right"/>} label={<span class="ticket-provider-settings__connection-copy"><strong>{connection.name??connection.id}{connection.default&&<small>Default</small>}</strong><small>{providerName(connection.provider as ExternalProviderKind)} · {connection.locator}</small></span>}/>)}</div>}</section>{error&&!setupOpen&&<p class="ticket-provider-settings__error" role="alert">{error}</p>}<p>Connection metadata is stored in <code>providers.json</code>; credentials remain in the OS keychain.</p></div>}
