export type Capabilities = Record<'create'|'update'|'close'|'notes'|'attachments'|'assignment'|'review_requests'|'dependencies'|'up_next'|'close_reasons'|'claims'|'atomic_batch'|'offline_mutation'|'history'|'watch'|'provider_idempotency', boolean> & {query_fields:string[]};
export interface ProviderDescriptor {connection_id:string;provider:string;display_name:string;locator:string;default:boolean;capabilities:Capabilities}
export interface ProviderConnection {id:string;provider:string;locator:string;name:string|null;default:boolean;settings:Record<string,unknown>}
export interface Ticket {qualified_id:string;native_id:string;native_url?:string;title:string;status:string;connection_id:string}
export class Api {
  constructor(private origin='',private secret=''){}
  private async request<T>(path:string,init:RequestInit={}):Promise<T>{const response=await fetch(`${this.origin}${path}`,{...init,headers:{'Content-Type':'application/json','X-Hotsheet-Secret':this.secret,...init.headers}});if(!response.ok)throw new Error((await response.json().catch(()=>null))?.error??`${response.status}`);return response.status===204?undefined as T:response.json()}
  providers=()=>this.request<ProviderDescriptor[]>('/providers');
  connections=()=>this.request<ProviderConnection[]>('/provider-connections');
  tickets=(id:string)=>this.request<Ticket[]>(`/providers/${encodeURIComponent(id)}/tickets`);
  createConnection=(value:ProviderConnection)=>this.request<ProviderConnection>('/provider-connections',{method:'POST',body:JSON.stringify(value)});
  updateConnection=(id:string,value:ProviderConnection)=>this.request<ProviderConnection>(`/provider-connections/${encodeURIComponent(id)}`,{method:'PATCH',body:JSON.stringify(value)});
  deleteConnection=(id:string)=>this.request<void>(`/provider-connections/${encodeURIComponent(id)}`,{method:'DELETE'});
  transfer=(kind:'copy'|'move',source:Ticket,destination_connection:string)=>this.request(`/provider-transfers/${kind}`,{method:'POST',body:JSON.stringify({source:{connection_id:source.connection_id,native_id:source.native_id},destination_connection,operation_id:crypto.randomUUID(),confirm:kind==='move'})});
}
