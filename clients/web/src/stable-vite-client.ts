/**
 * Stable-dev replacement for the small `/@vite/client` surface imported by Vite's CSS modules.
 * It installs the startup snapshot's styles but intentionally has no websocket, reconnect loop,
 * invalidation, or reload behavior. `npm run dev:hot` continues to use Vite's real client.
 */

interface StableHotContext {
  accept():void;
  prune(callback:()=>void):void;
}

const styles=new Map<string,HTMLStyleElement>();

export function createHotContext(ownerPath:string):StableHotContext {
  void ownerPath;
  return {accept(){/* Frozen snapshot: no updates arrive. */},prune(){/* No HMR disposal. */}};
}

export function updateStyle(id:string,content:string):void {
  let style=styles.get(id);
  if(!style){
    style=document.createElement('style');
    style.setAttribute('data-vite-dev-id',id);
    document.head.append(style);
    styles.set(id,style);
  }
  style.textContent=content;
}

export function removeStyle(id:string):void {
  const style=styles.get(id);
  style?.remove();
  styles.delete(id);
}
