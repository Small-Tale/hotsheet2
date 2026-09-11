import type { ServerCompatibility } from './compatibility';
import { prioritiesToWire } from './priority-wire';

export type Capabilities = Record<'create'|'update'|'close'|'notes'|'note_edit'|'note_delete'|'attachments'|'assignment'|'review_requests'|'dependencies'|'up_next'|'close_reasons'|'claims'|'atomic_batch'|'not_working_report'|'offline_mutation'|'history'|'watch'|'provider_idempotency', boolean> & {query_fields:string[]};
export interface ProviderDescriptor {connection_id:string;provider:string;display_name:string;locator:string;default:boolean;capabilities:Capabilities}
export interface ProviderConnection {id:string;provider:string;locator:string;name:string|null;default:boolean;settings:Record<string,unknown>}
export interface GitHubAuthStart {session_id:string;user_code:string;verification_uri:string;expires_in:number}
export type GitHubAuthStatus={state:'pending'}|{state:'authorized';credential_reference:string}|{state:'denied'|'expired'|'cancelled'}|{state:'error';message:string}
export interface Note {id:string;kind:'regular'|'activity'|'feedback_needed'|'feedback_draft'|'status';created_at:string;edited_at:string;summary?:string;text:string}
export interface MediaAnnotation {id:string;x:number;y:number;width:number;height:number;start_ms?:number;end_ms?:number;text:string}
export type AttachmentActorRole='human'|'ai'|'system'|'unknown';
export type AttachmentPurpose='problem_evidence'|'correctness_evidence'|'reference'|'other';
export interface AttachmentActor {identity?:string;display_name?:string;role:AttachmentActorRole}
export interface AttachmentMetadata {batch_id?:string;batch_label?:string;actor?:AttachmentActor;purpose?:AttachmentPurpose}
export interface Attachment extends AttachmentMetadata {id:string;filename:string;created_at:string;annotations?:MediaAnnotation[]}
export interface Ticket {qualified_id:string;native_id:string;native_url?:string;title:string;status:string;connection_id:string;notes?:Note[];attachments?:Attachment[]}
export interface CheckoutSource {connection_id:string;provider:string;locator:string}
export interface Checkout {id:string;root:string;alias:string;repository?:string;stores:string[];sources?:CheckoutSource[];default_source?:string}
export type TicketCloseReason='completed'|'not_planned'|'duplicate'|'obsolete';
export interface TicketRow {connection_id:string;native_id:string;qualified_id:string;id:string;slug:string;title:string;category?:string;priority?:string;status?:string;up_next:boolean;feedback_needed:boolean;tags:string[];blocked_by:string[];blocked_reason?:string;claimed_by?:string;claim_lease_expires_at?:string;worker_label?:string;claim_count:number;created_at?:string;updated_at?:string;completed_at?:string;closed_at?:string;close_reason?:TicketCloseReason;duplicate_of?:string;details?:string;notes?:Note[]}
export interface CheckoutTicketQuery {text?:string;status?:string;open?:boolean;up_next?:boolean;blocked?:boolean;compact?:boolean;tags?:string;has_attachment?:boolean;has_media_annotation?:boolean;has_commit?:boolean;attachment?:string;created_after?:string;created_before?:string;updated_after?:string;updated_before?:string;completed_after?:string;completed_before?:string;verified_after?:string;verified_before?:string}
export interface CheckoutTicketCounts {total:number;queued:number;backlog:number;archive:number;open:number;up_next:number;active:number;started:number;completed_today:number}
export interface CheckoutTicketPage {items:TicketRow[];next_cursor?:string;counts:CheckoutTicketCounts}
export interface CorruptTicket {store:string;store_path:string;path:string;id?:string;slug?:string;error:string;error_code?:'invalid_ticket'|'upgrade_required'}
export interface FullTicket extends TicketRow {details:string;blocked_reason?:string;notes:Note[];attachments:Attachment[];concurrency_token?:string;warnings?:string[]}
export interface DuplicateBacklink {reference:string;project_id:string;project_name:string;connection_id:string;native_id:string;qualified_id:string;slug:string;title:string}
export interface DuplicateBacklinkProject {project_id:string;project_name:string}
export interface DuplicateBacklinkResponse {backlinks:DuplicateBacklink[];inaccessible_projects:DuplicateBacklinkProject[]}
export type RepositoryFileChange='added'|'copied'|'deleted'|'modified'|'renamed'|'type_changed'|'unmerged'|'untracked';
export interface RepositoryFile {path:string;original_path?:string;staged?:RepositoryFileChange;unstaged?:RepositoryFileChange;untracked:boolean;conflicted:boolean}
export type RepositoryPlatform='macos'|'windows'|'linux';
export interface RepositoryStatus {initialized?:boolean;branch?:string;upstream?:string;ahead:number;behind:number;staged:number;unstaged:number;untracked:number;conflicted:number;clean?:boolean;files?:RepositoryFile[];root?:string;platform?:RepositoryPlatform;commit_count?:number;commits?:CodeReviewCommit[];ranges?:CodeReviewRange[];difftool?:string;truncated?:boolean}
export interface RepositoryPage<T> {items:T[];next_cursor?:number|null}
export interface CodeReviewCommit {sha:string;short_sha:string;subject:string;body?:string;committed_at:string}
export interface CodeReviewRange {from:string;to:string;count:number}
export interface CodeReviewSummary {files:{total:number;docs:number;tests:number;source:number;other:number};tests_added:number;tests_modified:number}
export interface CodeReviewFile {path:string;original_path?:string;change:Exclude<RepositoryFileChange,'unmerged'|'untracked'>;category:'docs'|'tests'|'source'|'other'}
export interface CodeReview {commits:CodeReviewCommit[];ranges:CodeReviewRange[];difftool?:string;truncated:boolean;summary?:CodeReviewSummary;files?:CodeReviewFile[]}
export type CodeReviewTarget={mode:'commit';commit:string}|{mode:'range';from:string;to:string}|{mode:'compare';from:string;to:string}|{mode:'ticket_file';path:string}|{mode:'worktree_file';path:string;area:'staged'|'unstaged'};
export interface PermissionRequest {id:number;project?:string;connection:string;tool:string;action:string;always_allow_supported?:boolean}
export interface AiModelDescriptor {id:string;label:string;effort_levels?:string[]}
export interface AiToolDescriptor {id:string;display_name:string;models:AiModelDescriptor[];default_model?:string;default_effort?:string;actions?:Array<'change_model'|'change_effort'>}
export interface AiToolDefaults {tool:string;model?:string;effort?:string}
export interface ToolConnection {id:string;tool:string;project:string;source?:string;role:'main'|'worker'|'drivespawned';busy:boolean;actions?:Array<'send_turn'|'interrupt'|'close'>;session_id?:string;last_error?:string;model?:string;effort?:string}
export interface ToolSession {connection_id:string;tool:string;project:string;session_id:string;updated_at_ms:number}
export interface TerminalInfo {id:string;alive:boolean;busy:boolean;cwd?:string;link?:string;progress?:number}
export interface TerminalSettings {inherit_global_shell_history:boolean}
export interface TerminalRead extends TerminalInfo {scrollback:string}
export interface CommandDefinition {id:string;title:string;kind?:'program'|'shell'|'ai';program?:string;args?:string[];cwd?:string;group?:string;confirmation?:string;command?:string;prompt?:string;tool?:string;icon?:string;color?:string}
export interface CustomView {id:string;name:string;query:string}
export interface CommandOutputLine {seq:number;stream:string;text:string}
export interface CommandRun {id:string;command_id:string;state:'running'|'completed'|'failed'|'cancelled';exit_code?:number;output:CommandOutputLine[]}
export interface ActivityEvent {id:string;ts:string;tool:string;project?:string;ticket?:string;session?:string;kind:string;summary:string;detail?:unknown;importance:'low'|'normal'|'high'}
export type ClientTurnEvent=
  |{type:'output';content:string;truncated:boolean}
  |{type:'permission_asked';tool:string;summary:string}
  |{type:'usage';model?:string;tokens_in:number;tokens_out:number;cost_usd?:number}
  |{type:'native_activity';source:string;payload:unknown}
  |{type:'coalesced';total:number;kinds:Record<string,number>}
  |{type:'done';reason:'completed'|'failed'|'interrupted';exit_code?:number}
  |{type:string;[key:string]:unknown};
export interface TurnStreamEnvelope {connection_id:string;ticket?:string;event:ClientTurnEvent}
export interface ChangeEvent {cursor?:number;store:string;kind:string;id:string;slug:string;message?:string;activity?:ActivityEvent;turn?:TurnStreamEnvelope}
export interface PollResponse {cursor:number;events:ChangeEvent[];overflow:boolean}
export const turnStreamEvents=(response:PollResponse):TurnStreamEnvelope[]=>response.events.flatMap(event=>event.kind==='turn_event'&&event.turn?[event.turn]:[]);
export const encodeAttachmentFilename=(filename:string)=>encodeURIComponent(filename);
export class Api {
  constructor(private origin='',private secret=''){}
  private async request<T>(path:string,init:RequestInit={}):Promise<T>{const headers=new Headers(init.headers);headers.set('X-Hotsheet-Secret',this.secret);if(!(init.body instanceof FormData)&&!headers.has('Content-Type'))headers.set('Content-Type','application/json');const response=await fetch(`${this.origin}${path}`,{...init,headers});if(!response.ok)throw new Error((await response.json().catch(()=>null))?.error??`${response.status}`);return response.status===204?undefined as T:response.json()}
  compatibility=()=>this.request<ServerCompatibility>('/compatibility');
  providers=()=>this.request<ProviderDescriptor[]>('/providers');
  connections=()=>this.request<ProviderConnection[]>('/provider-connections');
  tickets=(id:string)=>this.request<Ticket[]>(`/providers/${encodeURIComponent(id)}/tickets`);
  createConnection=(value:ProviderConnection)=>this.request<ProviderConnection>('/provider-connections',{method:'POST',body:JSON.stringify(value)});
  updateConnection=(id:string,value:ProviderConnection)=>this.request<ProviderConnection>(`/provider-connections/${encodeURIComponent(id)}`,{method:'PATCH',body:JSON.stringify(value)});
  deleteConnection=(id:string)=>this.request<void>(`/provider-connections/${encodeURIComponent(id)}`,{method:'DELETE'});
  startGitHubAuth=(web_base='https://github.com')=>this.request<GitHubAuthStart>('/github-auth/device',{method:'POST',body:JSON.stringify({web_base})});
  waitGitHubAuth=(session:string)=>this.request<GitHubAuthStatus>(`/github-auth/device/${encodeURIComponent(session)}`);
  githubAuthRepositories=(session:string)=>this.request<{repositories:string[]}>(`/github-auth/device/${encodeURIComponent(session)}/repositories`);
  cancelGitHubAuth=(session:string)=>this.request<void>(`/github-auth/device/${encodeURIComponent(session)}`,{method:'DELETE'});
  addCheckoutSource=(checkout:string,connection:ProviderConnection,makeDefault=false)=>this.request<Checkout>(`/checkouts/${encodeURIComponent(checkout)}/sources/${encodeURIComponent(connection.id)}`,{method:'PUT',body:JSON.stringify({provider:connection.provider,locator:connection.locator,make_default:makeDefault})});
  setCheckoutDefaultSource=(checkout:string,connectionId:string|null)=>this.request<Checkout>(`/checkouts/${encodeURIComponent(checkout)}/default-source`,{method:'PUT',body:JSON.stringify({connection_id:connectionId})});
  transfer=(kind:'copy'|'move',source:Ticket,destination_connection:string)=>this.request(`/provider-transfers/${kind}`,{method:'POST',body:JSON.stringify({source:{connection_id:source.connection_id,native_id:source.native_id},destination_connection,operation_id:crypto.randomUUID(),confirm:kind==='move'})});
  copyAttachment=(source:{connection_id:string;native_id:string;attachment_id:string},destination:{connection_id:string;native_id:string})=>this.request<FullTicket>('/provider-attachments/copy',{method:'POST',body:JSON.stringify({source,destination})});
  reportNotWorking=(connection:string,id:string,note:string,files:readonly File[],expectedToken?:string)=>{const body=new FormData();if(note.trim())body.append('note',note.trim());if(expectedToken)body.append('expected_token',expectedToken);for(const file of files)body.append('evidence',file,file.name);return this.request<FullTicket>(`/providers/${encodeURIComponent(connection)}/tickets/${encodeURIComponent(id)}/not-working`,{method:'POST',body});};
  checkoutTickets=(checkout:string,query?:string|CheckoutTicketQuery)=>{const options=typeof query==='string'?{text:query}:query??{},params=new URLSearchParams();for(const [key,value] of Object.entries(options)){if(value===undefined||value===''||(key==='text'&&typeof value==='string'&&!value.trim()))continue;params.set(key,key==='text'&&typeof value==='string'?value.trim():String(value))}const suffix=params.size?`?${params}`:'';return this.request<TicketRow[]>(`/checkouts/${encodeURIComponent(checkout)}/tickets${suffix}`)};
  checkoutTicketPage=(checkout:string,pageSize=200,cursor?:string,query:CheckoutTicketQuery={})=>{const params=new URLSearchParams({page_size:String(pageSize)});if(cursor)params.set('cursor',cursor);for(const [key,value] of Object.entries(query)){if(value===undefined||value===''||(key==='text'&&typeof value==='string'&&!value.trim()))continue;params.set(key,key==='text'&&typeof value==='string'?value.trim():String(value))}return this.request<CheckoutTicketPage>(`/checkouts/${encodeURIComponent(checkout)}/tickets?${params}`)};
  checkoutCorruptTickets=(checkout:string)=>this.request<CorruptTicket[]>(`/checkouts/${encodeURIComponent(checkout)}/corrupt-tickets`);
  createCorruptTicketRepair=(checkout:string,path:string)=>this.request<FullTicket>(`/checkouts/${encodeURIComponent(checkout)}/corrupt-tickets/repair`,{method:'POST',body:JSON.stringify({path})});
  checkoutTicket=(checkout:string,id:string)=>this.request<FullTicket&{store:string}>(`/checkouts/${encodeURIComponent(checkout)}/tickets/${encodeURIComponent(id)}`).then(ticket=>({store:ticket.store,ticket}));
  checkoutTicketDuplicateBacklinks=(checkout:string,id:string)=>this.request<DuplicateBacklinkResponse>(`/checkouts/${encodeURIComponent(checkout)}/tickets/${encodeURIComponent(id)}/duplicate-backlinks`);
  createCheckoutTicket=(checkout:string,value:{title:string;details?:string;category:string;priority?:string;status?:string;up_next?:boolean;tags?:string[]})=>this.request<FullTicket>(`/checkouts/${encodeURIComponent(checkout)}/tickets`,{method:'POST',body:JSON.stringify(prioritiesToWire(value))});
  updateCheckoutTicket=(checkout:string,id:string,value:Record<string,unknown>)=>this.request<FullTicket&{store:string}>(`/checkouts/${encodeURIComponent(checkout)}/tickets/${encodeURIComponent(id)}`,{method:'PATCH',body:JSON.stringify(prioritiesToWire(value))}).then(ticket=>({store:ticket.store,ticket}));
  closeCheckoutTicket=(checkout:string,id:string,reason:TicketCloseReason,duplicateOf?:string|{project_id:string;connection_id:string;native_id:string})=>this.request<FullTicket&{store:string}>(`/checkouts/${encodeURIComponent(checkout)}/tickets/${encodeURIComponent(id)}/close`,{method:'POST',body:JSON.stringify({reason,...(duplicateOf?{duplicate_of:duplicateOf}:{})})}).then(ticket=>({store:ticket.store,ticket}));
  batchUpdateCheckoutTickets=(checkout:string,updates:Array<{id:string;patch:Record<string,unknown>}>)=>this.request<Array<FullTicket&{store:string}>>(`/checkouts/${encodeURIComponent(checkout)}/batch`,{method:'POST',body:JSON.stringify({updates:updates.map(({id,patch})=>({id,...prioritiesToWire(patch)}))})});
  deleteCheckoutNote=(checkout:string,id:string,noteId:string)=>this.request<FullTicket&{store:string}>(`/checkouts/${encodeURIComponent(checkout)}/tickets/${encodeURIComponent(id)}/notes/${encodeURIComponent(noteId)}`,{method:'DELETE'}).then(ticket=>({store:ticket.store,ticket}));
  addCheckoutAttachment=(checkout:string,id:string,file:File,metadata:AttachmentMetadata={})=>{const headers:Record<string,string>={'Content-Type':file.type||'application/octet-stream','X-Hotsheet-Filename':encodeAttachmentFilename(file.name),'X-Hotsheet-Filename-Encoding':'percent','X-Hotsheet-Metadata-Encoding':'percent'};if(metadata.batch_id)headers['X-Hotsheet-Attachment-Batch']=encodeURIComponent(metadata.batch_id);if(metadata.batch_label)headers['X-Hotsheet-Attachment-Batch-Label']=encodeURIComponent(metadata.batch_label);if(metadata.actor?.role)headers['X-Hotsheet-Actor-Role']=metadata.actor.role;if(metadata.actor?.identity)headers['X-Hotsheet-Actor-Identity']=encodeURIComponent(metadata.actor.identity);if(metadata.actor?.display_name)headers['X-Hotsheet-Actor-Name']=encodeURIComponent(metadata.actor.display_name);if(metadata.purpose)headers['X-Hotsheet-Attachment-Purpose']=metadata.purpose;return this.request<FullTicket&{store:string}>(`/checkouts/${encodeURIComponent(checkout)}/tickets/${encodeURIComponent(id)}/attachments`,{method:'POST',headers,body:file}).then(ticket=>({store:ticket.store,ticket}));};
  updateCheckoutAttachmentMetadata=(checkout:string,id:string,attachment_ids:string[],metadata:AttachmentMetadata)=>this.request<FullTicket&{store:string}>(`/checkouts/${encodeURIComponent(checkout)}/tickets/${encodeURIComponent(id)}/attachments`,{method:'PATCH',body:JSON.stringify({attachment_ids,...metadata})}).then(ticket=>({store:ticket.store,ticket}));
  renameCheckoutAttachment=(checkout:string,id:string,attachmentId:string,filename:string)=>this.request<FullTicket&{store:string}>(this.checkoutAttachmentUrl(checkout,id,attachmentId).slice(this.origin.length),{method:'PATCH',body:JSON.stringify({filename})}).then(ticket=>({store:ticket.store,ticket}));
  checkoutAttachmentUrl=(checkout:string,id:string,attachmentId:string)=>`${this.origin}/checkouts/${encodeURIComponent(checkout)}/tickets/${encodeURIComponent(id)}/attachments/${encodeURIComponent(attachmentId)}`;
  checkoutAttachmentThumbnailUrl=(checkout:string,id:string,attachmentId:string)=>`${this.checkoutAttachmentUrl(checkout,id,attachmentId)}/thumbnail`;
  checkoutAttachmentByNameUrl=(checkout:string,id:string,filename:string)=>`${this.origin}/checkouts/${encodeURIComponent(checkout)}/tickets/${encodeURIComponent(id)}/attachments/by-name/${encodeURIComponent(filename)}`;
  checkoutAttachmentAction=(checkout:string,id:string,attachmentId:string,action:'open'|'reveal'|'path')=>this.request<{path:string}>(`/checkouts/${encodeURIComponent(checkout)}/tickets/${encodeURIComponent(id)}/attachments/${encodeURIComponent(attachmentId)}/action`,{method:'POST',body:JSON.stringify({action})});
  checkoutAttachmentByNameAction=(checkout:string,id:string,filename:string,action:'open'|'reveal'|'path')=>this.request<{path:string}>(`/checkouts/${encodeURIComponent(checkout)}/tickets/${encodeURIComponent(id)}/attachments/by-name/${encodeURIComponent(filename)}/action`,{method:'POST',body:JSON.stringify({action})});
  deleteCheckoutAttachment=(checkout:string,id:string,attachmentId:string)=>this.request<FullTicket&{store:string}>(this.checkoutAttachmentUrl(checkout,id,attachmentId).slice(this.origin.length),{method:'DELETE'}).then(ticket=>({store:ticket.store,ticket}));
  updateCheckoutAttachmentAnnotations=(checkout:string,id:string,attachmentId:string,annotations:MediaAnnotation[])=>this.request<FullTicket&{store:string}>(this.checkoutAttachmentUrl(checkout,id,attachmentId).slice(this.origin.length),{method:'PUT',body:JSON.stringify({annotations})}).then(ticket=>({store:ticket.store,ticket}));
  repositoryStatus=(checkout:string)=>this.request<RepositoryStatus>(`/checkouts/${encodeURIComponent(checkout)}/repository/status`);
  initializeRepository=(checkout:string)=>this.request<RepositoryStatus>(`/checkouts/${encodeURIComponent(checkout)}/repository/init`,{method:'POST'});
  configureRepositoryRemote=(checkout:string,remote:string)=>this.request<RepositoryStatus>(`/checkouts/${encodeURIComponent(checkout)}/repository/remote`,{method:'POST',body:JSON.stringify({remote})});
  repositoryFiles=(checkout:string,view:'staged'|'unstaged'|'untracked'|'conflicted',cursor=0,limit=50)=>this.request<RepositoryPage<RepositoryFile>>(`/checkouts/${encodeURIComponent(checkout)}/repository/files?view=${view}&cursor=${cursor}&limit=${limit}`);
  repositoryCommits=(checkout:string,cursor=0,limit=50)=>this.request<RepositoryPage<CodeReviewCommit>>(`/checkouts/${encodeURIComponent(checkout)}/repository/commits?cursor=${cursor}&limit=${limit}`);
  repositoryFileAction=(checkout:string,path:string,action:'open'|'reveal')=>this.request<void>(`/checkouts/${encodeURIComponent(checkout)}/repository/files/action`,{method:'POST',body:JSON.stringify({path,action})});
  openRepositoryReview=(checkout:string,target:CodeReviewTarget)=>this.request<void>(`/checkouts/${encodeURIComponent(checkout)}/repository/review`,{method:'POST',body:JSON.stringify(target)});
  codeReview=(checkout:string,id:string)=>this.request<CodeReview>(`/checkouts/${encodeURIComponent(checkout)}/tickets/${encodeURIComponent(id)}/code-review`);
  openCodeReview=(checkout:string,id:string,target:CodeReviewTarget)=>this.request<void>(`/checkouts/${encodeURIComponent(checkout)}/tickets/${encodeURIComponent(id)}/code-review`,{method:'POST',body:JSON.stringify(target)});
  permissions=()=>this.request<PermissionRequest[]>('/permissions');
  activeToolConnections=()=>this.request<ToolConnection[]>('/connections');
  aiTools=(refresh=false)=>this.request<AiToolDescriptor[]>(`/ai-tools${refresh?'?refresh=true':''}`);
  aiSettings=()=>this.request<AiToolDefaults>('/ai-settings');
  saveAiSettings=(value:AiToolDefaults)=>this.request<AiToolDefaults>('/ai-settings',{method:'PUT',body:JSON.stringify(value)});
  toolSessions=()=>this.request<ToolSession[]>('/drive/sessions');
  createToolConnection=(value:{tool:string;checkout:string;source?:string;connection_id?:string;session_id?:string;model?:string;effort?:string})=>this.request<ToolConnection>('/drive/connections',{method:'POST',body:JSON.stringify(value)});
  sendToolTurn=(id:string,content:string,session_id?:string,selection?:{model?:string;effort?:string})=>this.request<ToolConnection>(`/drive/connections/${encodeURIComponent(id)}/turns`,{method:'POST',body:JSON.stringify({content,...(session_id?{session_id}:{}),...selection})});
  interruptToolTurn=(id:string)=>this.request<ToolConnection>(`/drive/connections/${encodeURIComponent(id)}/interrupt`,{method:'POST'});
  deleteToolConnection=(checkout:string,id:string)=>this.request<void>(`/checkouts/${encodeURIComponent(checkout)}/drive/connections/${encodeURIComponent(id)}`,{method:'DELETE'});
  terminals=()=>this.request<TerminalInfo[]>('/terminals');
  terminalSettings=()=>this.request<TerminalSettings>('/terminal-settings');
  saveTerminalSettings=(value:TerminalSettings)=>this.request<TerminalSettings>('/terminal-settings',{method:'PUT',body:JSON.stringify(value)});
  terminal=(id:string)=>this.request<TerminalRead>(`/terminals/${encodeURIComponent(id)}`);
  createTerminal=(value:{id?:string;command?:string;args?:string[];cwd?:string;connect?:string;model?:string;effort?:string}={})=>this.request<TerminalInfo>('/terminals',{method:'POST',body:JSON.stringify(value)});
  deleteTerminal=(id:string)=>this.request<void>(`/terminals/${encodeURIComponent(id)}`,{method:'DELETE'});
  commands=()=>this.request<CommandDefinition[]>('/commands');
  saveCommands=(definitions:CommandDefinition[])=>this.request<CommandDefinition[]>('/commands',{method:'PUT',body:JSON.stringify(definitions)});
  customViews=()=>this.request<CustomView[]>('/views');
  saveCustomViews=(views:CustomView[])=>this.request<CustomView[]>('/views',{method:'PUT',body:JSON.stringify(views)});
  commandRuns=()=>this.request<CommandRun[]>('/command-runs');
  runCommand=(id:string)=>this.request<CommandRun>(`/commands/${encodeURIComponent(id)}/run`,{method:'POST'});
  commandRun=(id:string,after=0)=>this.request<CommandRun>(`/command-runs/${encodeURIComponent(id)}?after=${after}`);
  cancelCommandRun=(id:string)=>this.request<CommandRun>(`/command-runs/${encodeURIComponent(id)}/cancel`,{method:'POST'});
  pollEvents=(since?:number,signal?:AbortSignal,timeoutMs=25_000)=>this.request<PollResponse>(`/ws/poll?timeout_ms=${timeoutMs}${since===undefined?'':`&since=${since}`}`,{signal});
  resolvePermission=(id:number,decision:'allow'|'deny',scope:'once'|'always')=>this.request<{connection:string;decision:'allow'|'deny';persisted:boolean}>(`/permissions/${id}`,{method:'POST',body:JSON.stringify({decision,scope})});
}

export async function revealCorruptTicketFile(project:string,path:string):Promise<void>{
  const response=await fetch(`/__hotsheet/projects/${encodeURIComponent(project)}/corrupt-tickets/reveal`,{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({path})});
  if(!response.ok)throw new Error((await response.json().catch(()=>null) as {error?:string}|null)?.error??`${response.status}`);
}
