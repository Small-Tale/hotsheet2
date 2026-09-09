export type SearchDateField='created'|'completed'|'started'|'verified'|'archived'|'updated';
export type SearchDateDirection='before'|'after';

export type InlineSearchToken=
  |{kind:'tag';value:string;raw:string;label:string}
  |{kind:'has-attachment';value:'true';raw:string;label:string}
  |{kind:'attachment';value:string;raw:string;label:string}
  |{kind:'date';value:string;field:SearchDateField;direction:SearchDateDirection;raw:string;label:string};

const dateFields:readonly SearchDateField[]=['created','completed','started','verified','archived','updated'];
const unquote=(value:string)=>value.startsWith('"')&&value.endsWith('"')?value.slice(1,-1).replaceAll('\\"','"'):value;
const quoteIfNeeded=(value:string)=>/\s/.test(value)?`"${value.replaceAll('"','\\"')}"`:value;

export function parseSearchDate(value:string):string|undefined{
  const match=value.trim().match(/^(\d{1,2})-(\d{1,2})-(\d{4})(?:\s+(\d{1,2}):(\d{2})\s*(AM|PM))?$/i);
  if(!match)return undefined;
  const [,month,day,year,hourText,minuteText,meridiem]=match;
  let hour=hourText?Number(hourText):0;
  const minute=minuteText?Number(minuteText):0;
  if(Number(month)<1||Number(month)>12||Number(day)<1||Number(day)>31||hour>12||minute>59)return undefined;
  if(meridiem){hour%=12;if(meridiem.toUpperCase()==='PM')hour+=12}
  const date=new Date(Number(year),Number(month)-1,Number(day),hour,minute);
  if(date.getFullYear()!==Number(year)||date.getMonth()!==Number(month)-1||date.getDate()!==Number(day))return undefined;
  return date.toISOString();
}

export function tokenFromRaw(raw:string):InlineSearchToken|undefined{
  const source=raw.trim();
  if(source.toLowerCase()==='has-attachment')return{kind:'has-attachment',value:'true',raw:'has-attachment',label:'has attachment'};
  const tag=source.match(/^tag:("(?:\\.|[^"])*"|\S+)$/i);
  if(tag){const value=unquote(tag[1]);return value?{kind:'tag',value,raw:`tag:${quoteIfNeeded(value)}`,label:`tag:${value}`}:undefined}
  const attachment=source.match(/^attachment:("(?:\\.|[^"])*"|\S+)$/i);
  if(attachment){const value=unquote(attachment[1]);return value?{kind:'attachment',value,raw:`attachment:${quoteIfNeeded(value)}`,label:`attachment:${value}`}:undefined}
  const date=source.match(new RegExp(`^(${dateFields.join('|')})-(before|after):(.+)$`,'i'));
  if(date){const value=parseSearchDate(date[3]);if(!value)return undefined;const field=date[1].toLowerCase() as SearchDateField,direction=date[2].toLowerCase() as SearchDateDirection;return{kind:'date',field,direction,value,raw:`${field}-${direction}:${date[3].trim()}`,label:`${field} ${direction} ${date[3].trim()}`}}
  return undefined;
}

/** Consume one complete special token at the end of the input, leaving ordinary text intact. */
export function consumeSearchToken(input:string,force=false):{text:string;token?:InlineSearchToken}{
  const complete=force||/\s$/.test(input);
  const trimmed=input.trimEnd();
  const starts=[...trimmed.matchAll(/(?:^|\s)(tag:|has-attachment|attachment:|(?:created|completed|started|verified|archived|updated)-(?:before|after):)/gi)];
  const start=starts.at(-1)?.index;
  if(start===undefined)return{text:input};
  const raw=trimmed.slice(start).trimStart();
  const quotedComplete=!raw.includes('"')||/(?<!\\)"$/.test(raw);
  const token=quotedComplete&&complete?tokenFromRaw(raw):undefined;
  return token?{text:trimmed.slice(0,start).trimEnd(),token}:{text:input};
}

export function activeTagPrefix(input:string):string|undefined{
  const match=input.match(/(?:^|\s)tag:(?:"([^"]*)|([^\s]*))$/i);
  if(!match)return undefined;
  return match[1]||match[2]||'';
}

export function activeDatePrefix(input:string):`${SearchDateField}-${SearchDateDirection}`|undefined{
  const match=input.match(/(?:^|\s)((?:created|completed|started|verified|archived|updated)-(?:before|after)):[^\s]*$/i);
  if(!match)return undefined;
  return match[1].toLowerCase() as `${SearchDateField}-${SearchDateDirection}`;
}

export function tokenQuery(tokens:readonly InlineSearchToken[]){
  const query:Record<string,string|boolean>={};
  const tags=tokens.filter((token):token is Extract<InlineSearchToken,{kind:'tag'}>=>token.kind==='tag').map(token=>token.value);
  if(tags.length)query.tags=tags.join(',');
  if(tokens.some(token=>token.kind==='has-attachment'))query.has_attachment=true;
  const attachment=[...tokens].reverse().find((token):token is Extract<InlineSearchToken,{kind:'attachment'}>=>token.kind==='attachment');
  if(attachment)query.attachment=attachment.value;
  for(const token of tokens){if(token.kind!=='date')continue;const field=token.field==='started'||token.field==='archived'?'updated':token.field;query[`${field}_${token.direction}`]=token.value;if(token.field==='started')query.status='started';if(token.field==='archived')query.status='archive'}
  return query;
}
