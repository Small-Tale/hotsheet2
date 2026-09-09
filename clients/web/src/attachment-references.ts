export interface AttachmentReference { ticket?: string; filename: string; id?: string }
export interface AttachmentReferenceContext { baseUrl?: string; checkout: string; ticket: string; attachments?:readonly {ticket?:string;filename:string;id?:string}[] }

const IMAGE_EXTENSIONS = new Set(['avif','bmp','gif','ico','jpeg','jpg','png','svg','webp']);
const VIDEO_EXTENSIONS = new Set(['m4v','mov','mp4','ogv','webm']);

export function isImageAttachment(filename:string):boolean {
  const extension=filename.split('.').pop()?.toLocaleLowerCase();
  return Boolean(extension&&IMAGE_EXTENSIONS.has(extension));
}

export function isVideoAttachment(filename:string):boolean {
  const extension=filename.split('.').pop()?.toLocaleLowerCase();
  return Boolean(extension&&VIDEO_EXTENSIONS.has(extension));
}

export function isGalleryMediaAttachment(filename:string):boolean {
  return isImageAttachment(filename)||isVideoAttachment(filename);
}

export function parseAttachmentReference(value:string):AttachmentReference|undefined {
  const match=/^attachment:(?:\[([^\]]+)\])?(.+)$/.exec(value.trim());
  const filename=match?.[2]?.trim();
  if(!filename)return undefined;
  return{ticket:match?.[1]?.trim()||undefined,filename};
}

export function attachmentReferenceUrl(context:AttachmentReferenceContext,reference:AttachmentReference):string {
  const base=context.baseUrl??'';
  const ticket=reference.ticket||context.ticket,attachment=reference.id?encodeURIComponent(reference.id):`by-name/${encodeURIComponent(reference.filename)}`;
  return `${base}/checkouts/${encodeURIComponent(context.checkout)}/tickets/${encodeURIComponent(ticket)}/attachments/${attachment}`;
}

function resolveKnownAttachment(context:AttachmentReferenceContext,reference:AttachmentReference):AttachmentReference {
  const ticket=reference.ticket??context.ticket;
  const matched=context.attachments
    ?.filter(item=>(item.ticket??context.ticket)===ticket&&reference.filename.startsWith(item.filename))
    .sort((a,b)=>b.filename.length-a.filename.length)[0];
  if(matched)return{ticket:reference.ticket,filename:matched.filename,id:matched.id};
  const proseFallback=reference.filename.replace(/[.,;:!?]+$/,'');
  return proseFallback?{ticket:reference.ticket,filename:proseFallback}:reference;
}

export function expandAttachmentReferences(source:string,context?:AttachmentReferenceContext):string {
  if(!context)return source;
  const resolve=(raw:string,label?:string,image=false,autoPreview=true)=>{
    const parsed=parseAttachmentReference(raw),reference=parsed&&resolveKnownAttachment(context,parsed);
    if(!reference)return undefined;
    const url=attachmentReferenceUrl(context,reference),text=label||reference.filename,suffix=parsed.filename.slice(reference.filename.length),canonical=`attachment:${reference.ticket?`[${reference.ticket}]`:''}${reference.filename}`;
    const link=image||(autoPreview&&isImageAttachment(reference.filename))?`![${text}](${url} "${canonical}")`:`[${text}](${url} "${canonical}")`;
    return `${link}${suffix}`;
  };
  return source
    .replace(/(^|\s)(attachment:(?:\[[^\]]+\])?[A-Za-z0-9_.@+()-]+)/gm,(whole,prefix:string,raw:string)=>`${prefix}${resolve(raw)??raw}`)
    .replace(/`(attachment:(?:\[[^\]]+\])?[^`\n]+)`/g,(whole,raw:string)=>resolve(raw)??whole)
    .replace(/(!?)\[([^\]]*)\]\((attachment:(?:\[[^\]]+\])?[^)]+)\)/g,(whole,bang:string,label:string,raw:string)=>resolve(raw,label,bang==='!',false)??whole);
}

export function attachmentReferences(source:string,context?:AttachmentReferenceContext):AttachmentReference[] {
  const references:AttachmentReference[]=[];
  const seen=new Set<string>();
  const add=(raw:string)=>{const parsed=parseAttachmentReference(raw),resolved=parsed&&context?resolveKnownAttachment(context,parsed):parsed;if(!resolved)return;const reference:AttachmentReference=resolved.ticket?{ticket:resolved.ticket,filename:resolved.filename}:{filename:resolved.filename},key=`${reference.ticket??''}\0${reference.filename}`;if(!seen.has(key)){seen.add(key);references.push(reference)}};
  for(const match of source.matchAll(/(?:^|\s)(attachment:(?:\[[^\]]+\])?[A-Za-z0-9_.@+()-]+)/gm))add(match[1]);
  for(const match of source.matchAll(/`(attachment:(?:\[[^\]]+\])?[^`\n]+)`/g))add(match[1]);
  for(const match of source.matchAll(/!?\[[^\]]*\]\((attachment:(?:\[[^\]]+\])?[^)]+)\)/g))add(match[1]);
  return references;
}
