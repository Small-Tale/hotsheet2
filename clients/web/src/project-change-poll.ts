import type { Api,ChangeEvent, PollResponse } from './api';

const TICKET_CHANGE_KINDS = new Set([
  'attachment_added', 'attachment_removed', 'assigned', 'changed', 'claimed',
  'closed', 'created', 'deleted', 'moved', 'released', 'renewed', 'updated',
]);

export interface ProjectChangePollOptions {
  client: Pick<Api, 'pollEvents'>;
  refresh(): Promise<void>;
  beforeRefresh?(): Promise<void>;
  shouldRefresh?(response: PollResponse): boolean;
  onEvents?(response: PollResponse): Promise<void>;
  onError?(reason: unknown): void;
  retryMs?: number;
  maxRetryMs?: number;
  wait?: (milliseconds: number, signal: AbortSignal) => Promise<void>;
}

export interface ProjectChangeStreamOptions extends Omit<ProjectChangePollOptions,'client'> {
  client:Pick<Api,'pollEvents'> & Partial<Pick<Api,'changeWebSocketUrl'>>;
  openWebSocket?:(url:string)=>WebSocket;
  webSocketOpenTimeoutMs?:number;
}

export const containsTicketChange = (response: PollResponse): boolean =>
  response.overflow || response.events.some(event => TICKET_CHANGE_KINDS.has(event.kind));

export const containsRepositoryChange = (response: PollResponse,checkoutId:string): boolean =>
  response.events.some(event=>event.kind==='repository_changed'&&event.id===checkoutId);

const abortableWait = (milliseconds: number, signal: AbortSignal): Promise<void> => new Promise(resolve => {
  const timeout = window.setTimeout(resolve, milliseconds);
  signal.addEventListener('abort', () => { window.clearTimeout(timeout); resolve(); }, { once: true });
});

const wasAborted = (signal: AbortSignal): boolean => signal.aborted;

/** Start one replay-safe long-poll loop. The returned function aborts it permanently. */
export function startProjectChangePoll(options: ProjectChangePollOptions): () => void {
  const controller = new AbortController();
  const wait = options.wait ?? abortableWait;
  const retryMs = options.retryMs ?? 500;
  const maxRetryMs = options.maxRetryMs ?? 30_000;
  void (async () => {
    let cursor: number | undefined;
    let reconnecting = false;
    let retryDelay = retryMs;
    for (;;) {
      let response: PollResponse;
      try {
        response = await options.client.pollEvents(cursor, controller.signal);
      } catch (reason) {
        if (wasAborted(controller.signal)) return;
        options.onError?.(reason);
        // A failure is not itself a ticket invalidation. In particular, an older
        // server may not implement polling at all. Remember the outage and
        // reconcile once polling successfully reconnects instead of re-rendering
        // the workspace on every retry.
        reconnecting = true;
        cursor = undefined;
        await wait(retryDelay, controller.signal);
        retryDelay = Math.min(Math.max(retryDelay * 2, retryMs), maxRetryMs);
        if (wasAborted(controller.signal)) return;
        continue;
      }
      if (controller.signal.aborted) return;
      retryDelay = retryMs;
      const handshake = cursor === undefined;
      cursor = response.cursor;
      if (!handshake && response.events.length && options.onEvents) await options.onEvents(response).catch((reason: unknown) => { options.onError?.(reason); });
      const reconnect = handshake && reconnecting;
      const candidate = reconnect || (!handshake && containsTicketChange(response));
      reconnecting = false;
      if (candidate) {
        await options.beforeRefresh?.().catch((reason: unknown) => { options.onError?.(reason); });
        if (wasAborted(controller.signal)) return;
        const reconcile = reconnect || (options.shouldRefresh?.(response) ?? containsTicketChange(response));
        if (!reconcile) continue;
        await options.refresh().catch((reason: unknown) => { options.onError?.(reason); });
      }
    }
  })();
  return () => { controller.abort(); };
}

const changeEventFromMessage=async(data:unknown):Promise<ChangeEvent>=>{
  const text=typeof data==='string'?data:data instanceof Blob?await data.text():data instanceof ArrayBuffer?new TextDecoder().decode(data):ArrayBuffer.isView(data)?new TextDecoder().decode(data):'';
  const parsed:unknown=JSON.parse(text);
  if(!parsed||typeof parsed!=='object')throw new Error('Unsupported project change WebSocket payload.');
  const event=parsed as Partial<ChangeEvent>;
  if(typeof event.kind!=='string'||typeof event.store!=='string'||typeof event.id!=='string'||typeof event.slug!=='string')throw new Error('Unsupported project change WebSocket payload.');
  if(event.cursor!==undefined&&(!Number.isSafeInteger(event.cursor)||event.cursor<0))throw new Error('Invalid project change WebSocket cursor.');
  return event as ChangeEvent;
};

/**
 * Start the browser's WebSocket-first project change stream. A short poll handshake obtains the
 * replay cursor before the socket opens; another zero-wait poll after `open` closes the subscribe
 * race. Disconnects replay through long-poll before a bounded-backoff reconnect, so older servers
 * and environments that reject upgrades retain the complete long-poll behavior.
 */
export function startProjectChangeStream(options:ProjectChangeStreamOptions):()=>void{
  if(!options.client.changeWebSocketUrl||(!options.openWebSocket&&typeof WebSocket==='undefined'))return startProjectChangePoll(options);
  const controller=new AbortController(),wait=options.wait??abortableWait,retryMs=options.retryMs??500,maxRetryMs=options.maxRetryMs??30_000,openTimeoutMs=options.webSocketOpenTimeoutMs??2_000;
  const openSocket=options.openWebSocket??((url:string)=>new WebSocket(url));
  let socket:WebSocket|undefined;

  void(async()=>{
    let cursor:number|undefined,reconnecting=false,retryDelay=retryMs;
    const consume=async(response:PollResponse,forceRefresh=false)=>{
      if(wasAborted(controller.signal))return;
      if(response.events.length&&options.onEvents)await options.onEvents(response).catch((reason:unknown)=>{options.onError?.(reason)});
      const candidate=forceRefresh||containsTicketChange(response);
      if(!candidate)return;
      await options.beforeRefresh?.().catch((reason:unknown)=>{options.onError?.(reason)});
      if(wasAborted(controller.signal))return;
      const reconcile=forceRefresh||(options.shouldRefresh?.(response)??containsTicketChange(response));
      if(reconcile)await options.refresh().catch((reason:unknown)=>{options.onError?.(reason)});
    };
    const poll=async(since:number|undefined,timeoutMs?:number)=>options.client.pollEvents(since,controller.signal,timeoutMs);

    for(;;){
      if(wasAborted(controller.signal))return;
      if(cursor===undefined){
        try{
          const handshake=await poll(undefined);
          if(wasAborted(controller.signal))return;
          cursor=handshake.cursor;
          if(reconnecting)await consume(handshake,true);
          reconnecting=false;retryDelay=retryMs;
        }catch(reason){
          if(wasAborted(controller.signal))return;
          options.onError?.(reason);reconnecting=true;
          await wait(retryDelay,controller.signal);retryDelay=Math.min(Math.max(retryDelay*2,retryMs),maxRetryMs);
        }
        continue;
      }

      const outcome=await new Promise<{reason?:unknown}>(resolve=>{
        let activeSocket:WebSocket|undefined,ready=false,settled=false,failed=false;
        const queued:unknown[]=[];
        let processing=Promise.resolve();
        const finish=(reason?:unknown)=>{
          if(settled)return;
          settled=true;globalThis.clearTimeout(openTimer);controller.signal.removeEventListener('abort',abort);
          void processing.finally(()=> { resolve(reason?{reason}:{}); });
        };
        const fail=(reason:unknown)=>{
          if(failed)return;
          failed=true;options.onError?.(reason);
          const shouldClose=activeSocket?.readyState===0||activeSocket?.readyState===1;
          finish(reason);
          if(shouldClose)activeSocket?.close();
        };
        const accept=async(raw:unknown)=>{
          const event=await changeEventFromMessage(raw);
          if(event.cursor===undefined){
            if(event.kind!=='announce')throw new Error('Project change WebSocket event is not replayable.');
            await consume({cursor:cursor!,events:[event],overflow:false});
            return;
          }
          if(event.cursor<=cursor!)return;
          if(event.cursor!==cursor!+1)throw new Error(`Project change WebSocket cursor jumped from ${cursor} to ${event.cursor}.`);
          cursor=event.cursor;
          retryDelay=retryMs;
          await consume({cursor,events:[event],overflow:false});
        };
        const enqueue=(raw:unknown)=>{processing=processing.then(()=>accept(raw)).catch(fail)};
        const abort=()=>{activeSocket?.close();finish()};
        const openTimer=globalThis.setTimeout(()=> { fail(new Error('Project change WebSocket handshake timed out.')); },openTimeoutMs);
        controller.signal.addEventListener('abort',abort,{once:true});
        try{activeSocket=openSocket(options.client.changeWebSocketUrl!());socket=activeSocket}catch(reason){fail(reason);return}
        activeSocket.addEventListener('open',()=>{
          globalThis.clearTimeout(openTimer);
          processing=processing.then(async()=>{
            const catchup=await poll(cursor,0);
            cursor=catchup.cursor;await consume(catchup);
            ready=true;for(const raw of queued.splice(0))await accept(raw);
          }).catch(fail);
        },{once:true});
        activeSocket.addEventListener('message',event=>{if(ready)enqueue(event.data);else queued.push(event.data)});
        activeSocket.addEventListener('error',()=> { fail(new Error('Project change WebSocket failed.')); },{once:true});
        activeSocket.addEventListener('close',()=> { finish(); },{once:true});
      });
      socket=undefined;
      if(wasAborted(controller.signal))return;
      reconnecting=true;
      if(!outcome.reason)options.onError?.(new Error('Project change WebSocket closed.'));
      const fallbackStarted=Date.now();
      try{
        const replay=await poll(cursor,retryDelay);
        cursor=replay.cursor;await consume(replay,true);
      }catch(reason){
        options.onError?.(reason);cursor=undefined;
      }
      if(wasAborted(controller.signal))return;
      const remaining=Math.max(0,retryDelay-(Date.now()-fallbackStarted));
      if(remaining)await wait(remaining,controller.signal);
      retryDelay=Math.min(Math.max(retryDelay*2,retryMs),maxRetryMs);
    }
  })();
  return()=>{controller.abort();socket?.close()};
}
