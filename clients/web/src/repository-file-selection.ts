export function updateRepositoryFileSelection(current:readonly string[],visible:readonly string[],path:string,{additive=false,range=false,anchor}:{additive?:boolean;range?:boolean;anchor?:string}={}):string[]{
  if(range&&anchor){const from=visible.indexOf(anchor),to=visible.indexOf(path);if(from>=0&&to>=0){const selected=visible.slice(Math.min(from,to),Math.max(from,to)+1);return additive?[...new Set([...current,...selected])]:selected}}
  if(additive)return current.includes(path)?current.filter(item=>item!==path):[...current,path];
  return [path];
}
