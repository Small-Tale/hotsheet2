export interface RefreshBarrier {
  begin():()=>void;
  wait():Promise<void>;
}

export function createRefreshBarrier():RefreshBarrier {
  const pending=new Set<Promise<void>>();
  return {
    begin(){
      let release!:()=>void;
      const operation=new Promise<void>(resolve=>{release=resolve});
      pending.add(operation);
      let released=false;
      return ()=>{
        if(released)return;
        released=true;
        pending.delete(operation);
        release();
      };
    },
    wait(){return Promise.all([...pending]).then(()=>undefined)},
  };
}
