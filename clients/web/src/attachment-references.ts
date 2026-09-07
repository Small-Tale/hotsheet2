export interface AttachmentReference { ticket?: string; filename: string }
export interface AttachmentReferenceContext { baseUrl?: string; checkout: string; ticket: string; attachments?:readonly {ticket?:string;filename:string}[] }

const IMAGE_EXTENSIONS = new Set(['avif','bmp','gif','ico','jpeg','jpg','png','svg','webp']);

export function isImageAttachment(filename:string):boolean {
  const extension=filename.split('.').pop()?.toLocaleLowerCase();
  return Boolean(extension&&IMAGE_EXTENSIONS.has(extension));
}

export function parseAttachmentReference(value:string):AttachmentReference|undefined {
  const match=/^attachment:(?:\[([^\]]+)\])?(.+)$/.exec(value.trim());
  const filename=match?.[2]?.trim();
  if(!filename)return undefined;
  return{ticket:match?.[1]?.trim()||undefined,filename};
}

export function attachmentReferenceUrl(context:AttachmentReferenceContext,reference:AttachmentReference):string {
  const base=context.baseUrl??'';
  return `${base}/checkouts/${encodeURIComponent(context.checkout)}/tickets/${encodeURIComponent(reference.ticket||context.ticket)}/attachments/by-name/${encodeURIComponent(reference.filename)}`;
}

function resolveKnownAttachment(context:AttachmentReferenceContext,reference:AttachmentReference):AttachmentReference {
  const ticket=reference.ticket??context.ticket;
  const filename=context.attachments
    ?.filter(item=>(item.ticket??context.ticket)===ticket&&reference.filename.startsWith(item.filename))
    .map(item=>item.filename)
    .sort((a,b)=>b.length-a.length)[0];
  if(filename)return{ticket:reference.ticket,filename};
  const proseFallback=reference.filename.replace(/[.,;:!?]+$/,'');
  return proseFallback?{ticket:reference.ticket,filename:proseFallback}:reference;
}

export function expandAttachmentReferences(source:string,context?:AttachmentReferenceContext):string {
  if(!context)return source;
  const resolve=(raw:string,label?:string,image=false)=>{
    const parsed=parseAttachmentReference(raw),reference=parsed&&resolveKnownAttachment(context,parsed);
    if(!reference)return undefined;
    const url=attachmentReferenceUrl(context,reference),text=label||reference.filename,suffix=parsed.filename.slice(reference.filename.length),canonical=`attachment:${reference.ticket?`[${reference.ticket}]`:''}${reference.filename}`;
    const link=image||isImageAttachment(reference.filename)?`![${text}](${url} "${canonical}")`:`[${text}](${url} "${canonical}")`;
    return `${link}${suffix}`;
  };
  return source
    .replace(/(^|\s)(attachment:(?:\[[^\]]+\])?[A-Za-z0-9_.@+()-]+)/gm,(whole,prefix:string,raw:string)=>`${prefix}${resolve(raw)??raw}`)
    .replace(/`(attachment:(?:\[[^\]]+\])?[^`\n]+)`/g,(whole,raw:string)=>resolve(raw)??whole)
    .replace(/(!?)\[([^\]]*)\]\((attachment:(?:\[[^\]]+\])?[^)]+)\)/g,(whole,bang:string,label:string,raw:string)=>resolve(raw,label,bang==='!')??whole);
}

export function attachmentReferences(source:string,context?:AttachmentReferenceContext):AttachmentReference[] {
  const references:AttachmentReference[]=[];
  const seen=new Set<string>();
  const add=(raw:string)=>{const parsed=parseAttachmentReference(raw),reference=parsed&&context?resolveKnownAttachment(context,parsed):parsed;if(!reference)return;const key=`${reference.ticket??''}\0${reference.filename}`;if(!seen.has(key)){seen.add(key);references.push(reference)}};
  for(const match of source.matchAll(/(?:^|\s)(attachment:(?:\[[^\]]+\])?[A-Za-z0-9_.@+()-]+)/gm))add(match[1]);
  for(const match of source.matchAll(/`(attachment:(?:\[[^\]]+\])?[^`\n]+)`/g))add(match[1]);
  for(const match of source.matchAll(/!?\[[^\]]*\]\((attachment:(?:\[[^\]]+\])?[^)]+)\)/g))add(match[1]);
  return references;
}
