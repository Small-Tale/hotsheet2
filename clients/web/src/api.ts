import type { ServerCompatibility } from './compatibility';
import { prioritiesToWire } from './priority-wire';

export type Capabilities = Record<'create'|'update'|'close'|'notes'|'note_edit'|'note_delete'|'attachments'|'assignment'|'review_requests'|'dependencies'|'up_next'|'close_reasons'|'claims'|'atomic_batch'|'offline_mutation'|'history'|'watch'|'provider_idempotency', boolean> & {query_fields:string[]};
export interface ProviderDescriptor {connection_id:string;provider:string;display_name:string;locator:string;default:boolean;capabilities:Capabilities}
export interface ProviderConnection {id:string;provider:string;locator:string;name:string|null;default:boolean;settings:Record<string,unknown>}
export interface Note {id:string;kind:'regular'|'activity'|'feedback_needed'|'feedback_draft'|'status';created_at:string;edited_at:string;text:string}
export interface Attachment {id:string;filename:string;created_at:string}
export interface Ticket {qualified_id:string;native_id:string;native_url?:string;title:string;status:string;connection_id:string;notes?:Note[];attachments?:Attachment[]}
export interface Checkout {id:string;root:string;alias:string;repository?:string;stores:string[]}
export interface TicketRow {connection_id:string;native_id:string;qualified_id:string;id:string;slug:string;title:string;category?:string;priority?:string;status?:string;up_next:boolean;feedback_needed:boolean;tags:string[];blocked_by:string[];claimed_by?:string;worker_label?:string;claim_count:number;created_at?:string;updated_at?:string;completed_at?:string}
export interface CorruptTicket {store:string;store_path:string;path:string;id?:string;slug?:string;error:string;error_code?:'invalid_ticket'|'upgrade_required'}
export interface FullTicket extends TicketRow {details:string;blocked_reason?:string;notes:Note[];attachments:Attachment[];concurrency_token?:string}
export interface RepositoryStatus {branch?:string;upstream?:string;ahead:number;behind:number;staged:number;unstaged:number;untracked:number;conflicted:number}
export interface PermissionRequest {id:number;connection:string;tool:string;action:string;always_allow_supported?:boolean}
export interface ToolConnection {id:string;tool:string;project:string;role:'main'|'worker'|'drivespawned';busy:boolean}
export interface ChangeEvent {store:string;kind:string;id:string;slug:string}
export interface PollResponse {cursor:number;events:ChangeEvent[];overflow:boolean}
export const encodeAttachmentFilename=(filename:string)=>encodeURIComponent(filename);
export class Api {
  constructor(private origin='',private secret=''){}
  private async request<T>(path:string,init:RequestInit={}):Promise<T>{const response=await fetch(`${this.origin}${path}`,{...init,headers:{'Content-Type':'application/json','X-Hotsheet-Secret':this.secret,...init.headers}});if(!response.ok)throw new Error((await response.json().catch(()=>null))?.error??`${response.status}`);return response.status===204?undefined as T:response.json()}
  compatibility=()=>this.request<ServerCompatibility>('/compatibility');
  providers=()=>this.request<ProviderDescriptor[]>('/providers');
  connections=()=>this.request<ProviderConnection[]>('/provider-connections');
  tickets=(id:string)=>this.request<Ticket[]>(`/providers/${encodeURIComponent(id)}/tickets`);
  createConnection=(value:ProviderConnection)=>this.request<ProviderConnection>('/provider-connections',{method:'POST',body:JSON.stringify(value)});
  updateConnection=(id:string,value:ProviderConnection)=>this.request<ProviderConnection>(`/provider-connections/${encodeURIComponent(id)}`,{method:'PATCH',body:JSON.stringify(value)});
  deleteConnection=(id:string)=>this.request<void>(`/provider-connections/${encodeURIComponent(id)}`,{method:'DELETE'});
  transfer=(kind:'copy'|'move',source:Ticket,destination_connection:string)=>this.request(`/provider-transfers/${kind}`,{method:'POST',body:JSON.stringify({source:{connection_id:source.connection_id,native_id:source.native_id},destination_connection,operation_id:crypto.randomUUID(),confirm:kind==='move'})});
  copyAttachment=(source:{connection_id:string;native_id:string;attachment_id:string},destination:{connection_id:string;native_id:string})=>this.request<FullTicket>('/provider-attachments/copy',{method:'POST',body:JSON.stringify({source,destination})});
  checkoutTickets=(checkout:string,text?:string)=>this.request<TicketRow[]>(`/checkouts/${encodeURIComponent(checkout)}/tickets${text?.trim()?`?text=${encodeURIComponent(text.trim())}`:''}`);
  checkoutCorruptTickets=(checkout:string)=>this.request<CorruptTicket[]>(`/checkouts/${encodeURIComponent(checkout)}/corrupt-tickets`);
  checkoutTicket=(checkout:string,id:string)=>this.request<FullTicket&{store:string}>(`/checkouts/${encodeURIComponent(checkout)}/tickets/${encodeURIComponent(id)}`).then(ticket=>({store:ticket.store,ticket}));
  createCheckoutTicket=(checkout:string,value:{title:string;details?:string;category:string;priority?:string;status?:string;up_next?:boolean;tags?:string[]})=>this.request<FullTicket>(`/checkouts/${encodeURIComponent(checkout)}/tickets`,{method:'POST',body:JSON.stringify(prioritiesToWire(value))});
  updateCheckoutTicket=(checkout:string,id:string,value:Record<string,unknown>)=>this.request<FullTicket&{store:string}>(`/checkouts/${encodeURIComponent(checkout)}/tickets/${encodeURIComponent(id)}`,{method:'PATCH',body:JSON.stringify(prioritiesToWire(value))}).then(ticket=>({store:ticket.store,ticket}));
  deleteCheckoutNote=(checkout:string,id:string,noteId:string)=>this.request<FullTicket&{store:string}>(`/checkouts/${encodeURIComponent(checkout)}/tickets/${encodeURIComponent(id)}/notes/${encodeURIComponent(noteId)}`,{method:'DELETE'}).then(ticket=>({store:ticket.store,ticket}));
  addCheckoutAttachment=(checkout:string,id:string,file:File)=>this.request<FullTicket&{store:string}>(`/checkouts/${encodeURIComponent(checkout)}/tickets/${encodeURIComponent(id)}/attachments`,{method:'POST',headers:{'Content-Type':file.type||'application/octet-stream','X-Hotsheet-Filename':encodeAttachmentFilename(file.name),'X-Hotsheet-Filename-Encoding':'percent'},body:file}).then(ticket=>({store:ticket.store,ticket}));
  checkoutAttachmentUrl=(checkout:string,id:string,attachmentId:string)=>`${this.origin}/checkouts/${encodeURIComponent(checkout)}/tickets/${encodeURIComponent(id)}/attachments/${encodeURIComponent(attachmentId)}`;
  deleteCheckoutAttachment=(checkout:string,id:string,attachmentId:string)=>this.request<FullTicket&{store:string}>(this.checkoutAttachmentUrl(checkout,id,attachmentId).slice(this.origin.length),{method:'DELETE'}).then(ticket=>({store:ticket.store,ticket}));
  repositoryStatus=(checkout:string)=>this.request<RepositoryStatus>(`/checkouts/${encodeURIComponent(checkout)}/repository/status`);
  permissions=()=>this.request<PermissionRequest[]>('/permissions');
  activeToolConnections=()=>this.request<ToolConnection[]>('/connections');
  pollEvents=(since?:number,signal?:AbortSignal,timeoutMs=25_000)=>this.request<PollResponse>(`/ws/poll?timeout_ms=${timeoutMs}${since===undefined?'':`&since=${since}`}`,{signal});
  resolvePermission=(id:number,decision:'allow'|'deny',scope:'once'|'always')=>this.request<{connection:string;decision:'allow'|'deny';persisted:boolean}>(`/permissions/${id}`,{method:'POST',body:JSON.stringify({decision,scope})});
}
