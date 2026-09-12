export interface TerminalProjectScope {id:string;root:string}

function normalizedRoot(root:string):string{return root.replace(/[\\/]+$/,'')||root}

function containsPath(root:string,path:string):boolean{
  const normalized=normalizedRoot(root);
  return path===normalized||path.startsWith(`${normalized}/`)||path.startsWith(`${normalized}\\`);
}

/** Assign a host-wide terminal to the most specific open project containing its cwd. */
export function terminalProjectOwner(projects:readonly TerminalProjectScope[],cwd?:string):string|undefined{
  if(!cwd)return projects.length===1?projects[0]?.id:undefined;
  return projects.filter(project=>containsPath(project.root,cwd)).sort((left,right)=>normalizedRoot(right.root).length-normalizedRoot(left.root).length)[0]?.id;
}

