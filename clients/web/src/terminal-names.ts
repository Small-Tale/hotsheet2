const ULID=/^[0-9A-HJKMNP-TV-Z]{26}$/i;

export function defaultTerminalName(id:string,index:number):string{
  if(ULID.test(id))return `Terminal ${index+1}`;
  const words=id.trim().split(/[-_\s]+/).filter(Boolean);
  if(words.length===0)return `Terminal ${index+1}`;
  return words.map(word=>word.length<=3&&word===word.toUpperCase()?word:word.charAt(0).toUpperCase()+word.slice(1)).join(' ');
}

export function terminalNameKey(projectId:string,terminalId:string):string{return `${projectId}:${terminalId}`}

export function parseTerminalNames(raw:string|null):Record<string,string>{
  if(!raw)return{};
  try{const value=JSON.parse(raw) as unknown;if(!value||typeof value!=='object'||Array.isArray(value))return{};return Object.fromEntries(Object.entries(value).filter((entry):entry is [string,string]=>typeof entry[1]==='string'&&Boolean(entry[1].trim())).map(([key,name])=>[key,name.trim()]))}catch{return{}}
}
