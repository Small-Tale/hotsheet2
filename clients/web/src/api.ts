export type Capabilities = Record<'create'|'update'|'close'|'notes'|'attachments'|'assignment'|'review_requests'|'dependencies'|'up_next'|'close_reasons'|'claims'|'atomic_batch'|'offline_mutation'|'history'|'watch'|'provider_idempotency', boolean> & {query_fields:string[]};
export interface ProviderDescriptor {connection_id:string;provider:string;display_name:string;locator:string;default:boolean;capabilities:Capabilities}
export interface ProviderConnection {id:string;provider:string;locator:string;name:string|null;default:boolean;settings:Record<string,unknown>}
export interface Note {id:string;kind:'regular'|'activity'|'feedback_needed'|'feedback_draft'|'status';created_at:string;edited_at:string;text:string}
export interface Attachment {id:string;filename:string;created_at:string}
export interface Ticket {qualified_id:string;native_id:string;native_url?:string;title:string;status:string;connection_id:string;notes?:Note[];attachments?:Attachment[]}
export interface Checkout {id:string;root:string;alias:string;repository?:string;stores:string[]}
export interface TicketRow {connection_id:string;native_id:string;qualified_id:string;id:string;slug:string;title:string;category?:string;priority?:string;status?:string;up_next:boolean;tags:string[];blocked_by:string[];claimed_by?:string;worker_label?:string;claim_count:number;created_at?:string;updated_at?:string;completed_at?:string}
export interface FullTicket extends TicketRow {details:string;blocked_reason?:string;notes:Note[];attachments:Attachment[];concurrency_token?:string}
export interface RepositoryStatus {branch?:string;upstream?:string;ahead:number;behind:number;staged:number;unstaged:number;untracked:number;conflicted:number}
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
  checkoutTickets=(checkout:string)=>this.request<TicketRow[]>(`/checkouts/${encodeURIComponent(checkout)}/tickets`);
  checkoutTicket=(checkout:string,id:string)=>this.request<FullTicket&{store:string}>(`/checkouts/${encodeURIComponent(checkout)}/tickets/${encodeURIComponent(id)}`).then(ticket=>({store:ticket.store,ticket}));
  createCheckoutTicket=(checkout:string,value:{title:string;details?:string;category:string;priority?:string;status?:string;up_next?:boolean;tags?:string[]})=>this.request<FullTicket>(`/checkouts/${encodeURIComponent(checkout)}/tickets`,{method:'POST',body:JSON.stringify(value)});
  updateCheckoutTicket=(checkout:string,id:string,value:Record<string,unknown>)=>this.request<FullTicket&{store:string}>(`/checkouts/${encodeURIComponent(checkout)}/tickets/${encodeURIComponent(id)}`,{method:'PATCH',body:JSON.stringify(value)}).then(ticket=>({store:ticket.store,ticket}));
  addCheckoutAttachment=(checkout:string,id:string,file:File)=>this.request<FullTicket&{store:string}>(`/checkouts/${encodeURIComponent(checkout)}/tickets/${encodeURIComponent(id)}/attachments`,{method:'POST',headers:{'Content-Type':file.type||'application/octet-stream','X-Hotsheet-Filename':file.name},body:file}).then(ticket=>({store:ticket.store,ticket}));
  repositoryStatus=(checkout:string)=>this.request<RepositoryStatus>(`/checkouts/${encodeURIComponent(checkout)}/repository/status`);
}
