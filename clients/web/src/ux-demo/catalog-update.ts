type OpenCatalogPopup=Element&{open:boolean};

/** Apply catalog metadata only after every native popup has finished closing so a
 * background development response cannot morph away user-owned popup state. */
export function applyAfterCatalogPopupsClose(root:ParentNode,apply:()=>void):void{
  const popup=[...root.querySelectorAll<OpenCatalogPopup>('wa-select,wa-dropdown')].find(item=>item.open);
  if(!popup){apply();return}
  popup.addEventListener('wa-after-hide',()=>{
    applyAfterCatalogPopupsClose(root,apply);
  },{once:true});
}
