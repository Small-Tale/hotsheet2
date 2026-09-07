import type { InspectorTab } from './components/ticket-inspector';
import type { InlineFeedbackReply } from './feedback-replies';
import type { TicketView } from './ticket-views';

export interface StoredPendingAttachment { id:string; name:string }
export interface ProjectWorkspaceSession {
  selectedView:TicketView;
  selectedTicketSlugs:string[];
  searchOpen:boolean;
  searchQuery:string;
  inspectorTab:InspectorTab;
  composer:{open:boolean;title:string;details:string;category:string;upNext:boolean;attachments:StoredPendingAttachment[]};
  composingNote:boolean;
  newNoteDraft:string;
  feedbackReplies:Record<string,InlineFeedbackReply[]>;
  feedbackSelections:Record<string,string[]>;
  feedbackNoteId?:string;
  feedbackDraft:string;
  notWorking?:{ticketId:string;slug:string;connectionId:string;note:string;attachments:StoredPendingAttachment[]};
}

const PREFIX='hotsheet.workspace.project-session.v1.';
const ACTIVE_ROOT='hotsheet.workspace.active-project-root.v1';
const DB_NAME='hotsheet-draft-files-v1',STORE='files';

const stringValue=(value:unknown,fallback='')=>typeof value==='string'?value:fallback;
const booleanValue=(value:unknown)=>value===true;
function recordValue(value:unknown):Record<string,unknown>{return value&&typeof value==='object'&&!Array.isArray(value)?value as Record<string,unknown>:{}}
function validAttachments(value:unknown):StoredPendingAttachment[]{return Array.isArray(value)?value.flatMap(item=>{const record=recordValue(item),id=stringValue(record.id),name=stringValue(record.name);return id&&name?[{id,name}]:[]}):[]}
function validRecord(value:unknown):Record<string,InlineFeedbackReply[]>{return Object.fromEntries(Object.entries(recordValue(value)).map(([key,replies])=>[key,Array.isArray(replies)?replies.flatMap(reply=>{const record=recordValue(reply);return Number.isFinite(record.offset)&&typeof record.text==='string'?[{offset:Number(record.offset),text:record.text}]:[]}):[]] as const))}
function validSelections(value:unknown):Record<string,string[]>{return Object.fromEntries(Object.entries(recordValue(value)).map(([key,items])=>[key,Array.isArray(items)?items.filter((item):item is string=>typeof item==='string'):[]] as const))}

export function loadProjectWorkspaceSession(storage:Pick<Storage,'getItem'>,projectId:string):ProjectWorkspaceSession|undefined{
  try{
    const value=recordValue(JSON.parse(storage.getItem(PREFIX+projectId)??'null'));
    const composer=recordValue(value.composer);
    if(Object.keys(value).length===0||Object.keys(composer).length===0)return undefined;
    const selectedView=stringValue(value.selectedView),inspectorTab=stringValue(value.inspectorTab),notWorking=recordValue(value.notWorking);
    return {
      selectedView:(['all','backlog','archive','errors'].includes(selectedView)?selectedView:'all') as TicketView,
      selectedTicketSlugs:Array.isArray(value.selectedTicketSlugs)?value.selectedTicketSlugs.filter((item):item is string=>typeof item==='string'):[],
      searchOpen:booleanValue(value.searchOpen),searchQuery:stringValue(value.searchQuery),
      inspectorTab:(['info','timeline','attachments','code-review'].includes(inspectorTab)?inspectorTab:'info') as InspectorTab,
      composer:{open:booleanValue(composer.open),title:stringValue(composer.title),details:stringValue(composer.details),category:stringValue(composer.category,'task'),upNext:booleanValue(composer.upNext),attachments:validAttachments(composer.attachments)},
      composingNote:booleanValue(value.composingNote),newNoteDraft:stringValue(value.newNoteDraft),
      feedbackReplies:validRecord(value.feedbackReplies),feedbackSelections:validSelections(value.feedbackSelections),
      feedbackNoteId:typeof value.feedbackNoteId==='string'?value.feedbackNoteId:undefined,feedbackDraft:stringValue(value.feedbackDraft),
      notWorking:Object.keys(notWorking).length?{ticketId:stringValue(notWorking.ticketId),slug:stringValue(notWorking.slug),connectionId:stringValue(notWorking.connectionId),note:stringValue(notWorking.note),attachments:validAttachments(notWorking.attachments)}:undefined,
    };
  }catch{return undefined}
}

export function saveProjectWorkspaceSession(storage:Pick<Storage,'setItem'>,projectId:string,value:ProjectWorkspaceSession){storage.setItem(PREFIX+projectId,JSON.stringify(value))}
export function activeProjectRoot(storage:Pick<Storage,'getItem'>){return storage.getItem(ACTIVE_ROOT)??''}
export function saveActiveProjectRoot(storage:Pick<Storage,'setItem'>,root:string){storage.setItem(ACTIVE_ROOT,root)}

function database():Promise<IDBDatabase>{return new Promise((resolve,reject)=>{const request=indexedDB.open(DB_NAME,1);request.onupgradeneeded=()=>{request.result.createObjectStore(STORE)};request.onsuccess=()=>{resolve(request.result)};request.onerror=()=>{reject(request.error??new Error('Could not open the draft file database.'))}})}
export async function saveDraftFile(scope:string,id:string,file:File){const db=await database();await transaction(db,'readwrite',store=>{store.put({blob:file,name:file.name,type:file.type,lastModified:file.lastModified},`${scope}:${id}`)});db.close()}
export async function loadDraftFiles(scope:string,items:readonly StoredPendingAttachment[]):Promise<Array<{id:string;name:string;file:File}>>{const db=await database();const loaded=await Promise.all(items.map(async item=>{const value=await requestValue<{blob:Blob;name:string;type:string;lastModified:number}>(db.transaction(STORE).objectStore(STORE).get(`${scope}:${item.id}`));return value?{id:item.id,name:value.name,file:new File([value.blob],value.name,{type:value.type,lastModified:value.lastModified})}:undefined}));db.close();return loaded.filter((item):item is {id:string;name:string;file:File}=>Boolean(item))}
export async function deleteDraftFiles(scope:string,ids:readonly string[]){if(!ids.length)return;const db=await database();await transaction(db,'readwrite',store=>{for(const id of ids)store.delete(`${scope}:${id}`)});db.close()}
function requestValue<T>(request:IDBRequest):Promise<T|undefined>{return new Promise((resolve,reject)=>{request.onsuccess=()=>{resolve(request.result as T|undefined)};request.onerror=()=>{reject(request.error??new Error('Could not read a draft file.'))}})}
function transaction(db:IDBDatabase,mode:IDBTransactionMode,action:(store:IDBObjectStore)=>void):Promise<void>{return new Promise((resolve,reject)=>{const tx=db.transaction(STORE,mode);action(tx.objectStore(STORE));tx.oncomplete=()=>{resolve()};tx.onerror=()=>{reject(tx.error??new Error('Could not update draft files.'))}})}
