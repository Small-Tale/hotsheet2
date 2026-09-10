export type SearchDateField='created'|'completed'|'started'|'verified'|'archived'|'updated';
export type SearchDateDirection='before'|'after';
export type SearchPresence='attachment'|'media-annotation'|'commit';
export type SearchLifecycle='up-next'|'active'|'open'|'closed'|'duplicate'|'not-started'|'started'|'completed'|'verified'|'backlog'|'backlogged'|'archived';

type InlineSearchTokenValue=
  |{kind:'tag';value:string;raw:string;label:string}
  |{kind:'has';value:SearchPresence;raw:string;label:string}
  |{kind:'attachment';value:string;raw:string;label:string}
  |{kind:'is';value:SearchLifecycle;raw:string;label:string}
  |{kind:'date';value:string;field:SearchDateField;direction:SearchDateDirection;raw:string;label:string};
export type InlineSearchToken=InlineSearchTokenValue&{offset?:number};
export type InlineSearchPart={kind:'text';value:string}|{kind:'token';token:InlineSearchToken};

const dateFields:readonly SearchDateField[]=['created','completed','started','verified','archived','updated'];
const unquote=(value:string)=>value.startsWith('"')&&value.endsWith('"')?value.slice(1,-1).replaceAll('\\"','"'):value;
const quoteIfNeeded=(value:string)=>/\s/.test(value)?`"${value.replaceAll('"','\\"')}"`:value;

function localizedDigits(locale?:string){const digits=new Map<string,string>();for(let value=0;value<10;value++)digits.set(new Intl.NumberFormat(locale,{useGrouping:false}).format(value),String(value));return digits}
function latinNumber(value:string,locale?:string){const digits=localizedDigits(locale);return Number(Array.from(value,character=>digits.get(character)??character).join(''))}
function localDateOrder(locale?:string){return new Intl.DateTimeFormat(locale,{year:'numeric',month:'numeric',day:'numeric'}).formatToParts(new Date(2006,10,22)).filter(part=>part.type==='year'||part.type==='month'||part.type==='day').map(part=>part.type as 'year'|'month'|'day')}
function localDayPeriods(locale?:string){const value=(hour:number)=>new Intl.DateTimeFormat(locale,{hour:'numeric',hour12:true}).formatToParts(new Date(2000,0,1,hour)).find(part=>part.type==='dayPeriod')?.value.toLocaleLowerCase(locale);return{am:value(1),pm:value(13)}}

function validLocalDate(year:number,month:number,day:number,hour:number,minute:number){
  if(month<1||month>12||day<1||day>31||hour<0||hour>23||minute<0||minute>59)return undefined;
  const date=new Date(year,month-1,day,hour,minute);
  return date.getFullYear()===year&&date.getMonth()===month-1&&date.getDate()===day?date:undefined;
}

export function parseSearchDate(value:string,locale?:string,now=new Date()):string|undefined{
  const source=value.trim(),iso=source.match(/^(\d{4})-(\d{2})-(\d{2})(?:[T ](\d{2}):(\d{2})(?::\d{2}(?:\.\d+)?)?(Z|[+-]\d{2}:?\d{2})?)?$/i);
  if(iso){
    if(iso[6]){if(!validLocalDate(Number(iso[1]),Number(iso[2]),Number(iso[3]),12,0)||Number(iso[4])>23||Number(iso[5])>59)return undefined;const timestamp=Date.parse(source);return Number.isNaN(timestamp)?undefined:new Date(timestamp).toISOString()}
    return validLocalDate(Number(iso[1]),Number(iso[2]),Number(iso[3]),Number(iso[4]||0),Number(iso[5]||0))?.toISOString();
  }
  const slashYmd=source.match(/^(\d{4})\/(\d{1,2})\/(\d{1,2})(?:\s+(\d{1,2}):(\d{2}))?$/);
  if(slashYmd)return validLocalDate(Number(slashYmd[1]),Number(slashYmd[2]),Number(slashYmd[3]),Number(slashYmd[4]||0),Number(slashYmd[5]||0))?.toISOString();
  const relative=source.match(/^(\d+(?:\.\d+)?)\s*(m|h|d|w)\s+ago$/i);
  if(relative){const units:{[key:string]:number}={m:60_000,h:3_600_000,d:86_400_000,w:604_800_000};return new Date(now.getTime()-Number(relative[1])*units[relative[2].toLowerCase()]).toISOString()}
  const numbers=[...source.matchAll(/\p{Number}+/gu)];
  if(numbers.length<3||numbers.length>5)return undefined;
  const order=localDateOrder(locale),parts=Object.fromEntries(order.map((key,index)=>[key,latinNumber(numbers[index][0],locale)])) as Record<'year'|'month'|'day',number>;
  let hour=numbers[3]?latinNumber(numbers[3][0],locale):0;const minute=numbers[4]?latinNumber(numbers[4][0],locale):0;
  const timeSuffix=numbers[3]?source.slice(numbers[3].index+numbers[3][0].length).toLocaleLowerCase(locale):'',periods=localDayPeriods(locale),hasAm=Boolean(periods.am&&timeSuffix.includes(periods.am)),hasPm=Boolean(periods.pm&&timeSuffix.includes(periods.pm));
  if(hasAm||hasPm){if(hour<1||hour>12)return undefined;hour%=12;if(hasPm)hour+=12}
  const date=validLocalDate(parts.year,parts.month,parts.day,hour,minute);
  if(!date)return undefined;
  return date.toISOString();
}

export function dateTokenFromInput(prefix:`${SearchDateField}-${SearchDateDirection}`,dateValue:string,timeValue='',locale?:string):InlineSearchToken|undefined{
  const [year,month,day]=dateValue.split('-').map(Number),[hour=0,minute=0]=timeValue.split(':').map(Number),date=validLocalDate(year,month,day,hour,minute);
  if(!date)return undefined;
  const [field,direction]=prefix.split('-') as [SearchDateField,SearchDateDirection],formatter=new Intl.DateTimeFormat(locale,timeValue?{dateStyle:'short',timeStyle:'short'}:{dateStyle:'short'}),raw=`${prefix}:${dateValue}${timeValue?`T${timeValue}`:''}`;
  return{kind:'date',field,direction,value:date.toISOString(),raw,label:`${field} ${direction} ${formatter.format(date)}`};
}

export function tokenFromRaw(raw:string):InlineSearchToken|undefined{
  const source=raw.trim();
  const lifecycle=source.match(/^is:(up-next|active|open|closed|duplicate|not-started|started|completed|verified|backlog|backlogged|archived)$/i);
  if(lifecycle){const value=lifecycle[1].toLowerCase() as SearchLifecycle;return{kind:'is',value,raw:`is:${value}`,label:`is:${value}`}}
  const presence=source.match(/^has:(attachment|media-annotation|commit)$/i);
  if(presence){const value=presence[1].toLowerCase() as SearchPresence;return{kind:'has',value,raw:`has:${value}`,label:`has ${value.replace('-',' ')}`}}
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
  const starts=[...trimmed.matchAll(/(?:^|\s)(is:|tag:|has:|attachment:|(?:created|completed|started|verified|archived|updated)-(?:before|after):)/gi)];
  const start=starts.at(-1)?.index;
  if(start===undefined)return{text:input};
  const raw=trimmed.slice(start).trimStart();
  const quotedComplete=!raw.includes('"')||/(?<!\\)"$/.test(raw);
  const token=quotedComplete&&complete?tokenFromRaw(raw):undefined;
  return token?{text:trimmed.slice(0,start).trimEnd(),token}:{text:input};
}

/** Consume every complete structured token, including tokens embedded in boolean text. */
export function consumeSearchTokens(input:string,force=false):{text:string;tokens:InlineSearchToken[];removed:ReadonlyArray<{start:number;end:number}>}{
  const pattern=/(?:^|[\s(])((?:is|tag|has|attachment):(?:"(?:\\.|[^"])*"|[^\s()]+)|(?:created|completed|started|verified|archived|updated)-(?:before|after):(?:"(?:\\.|[^"])*"|[^\s()]+(?:\s+ago)?))(?=$|[\s)])/gi,result:InlineSearchToken[]=[],removed:Array<{start:number;end:number}>=[];
  let text='',cursor=0;
  for(const match of input.matchAll(pattern)){const raw=match[1],start=match.index+match[0].lastIndexOf(raw),end=start+raw.length;if(end===input.length&&!force&&!/\s$/.test(input))continue;const token=tokenFromRaw(raw);if(!token)continue;text+=input.slice(cursor,start);result.push({...token,offset:text.length});removed.push({start,end});cursor=end}
  text+=input.slice(cursor);return{text,tokens:result,removed};
}

export function activeTagPrefix(input:string):string|undefined{
  const match=input.match(/(?:^|\s)tag:(?:"([^"]*)|([^\s]*))$/i);
  if(!match)return undefined;
  return match[1]||match[2]||'';
}

export function activeDatePrefix(input:string):`${SearchDateField}-${SearchDateDirection}`|undefined{
  const match=input.match(/(?:^|\s)((?:created|completed|started|verified|archived|updated)-(?:before|after)):.*$/i);
  if(!match)return undefined;
  return match[1].toLowerCase() as `${SearchDateField}-${SearchDateDirection}`;
}

/** Split the editable text around the committed, atomic tokens that live inside it. */
export function inlineSearchParts(input:string,tokens:readonly InlineSearchToken[]):InlineSearchPart[]{
  const ordered=tokens.map((token,index)=>({token,index,offset:Math.max(0,Math.min(input.length,token.offset??input.length))})).sort((left,right)=>left.offset-right.offset||left.index-right.index),parts:InlineSearchPart[]=[];
  let cursor=0;
  for(const {token,offset} of ordered){if(offset>cursor)parts.push({kind:'text',value:input.slice(cursor,offset)});parts.push({kind:'token',token:{...token,offset}});cursor=offset}
  if(cursor<input.length||parts.length===0)parts.push({kind:'text',value:input.slice(cursor)});
  return parts;
}

/** Rebuild the readable expression while retaining the visual order of committed tokens. */
export function orderedSearchText(input:string,tokens:readonly InlineSearchToken[],include:(token:InlineSearchToken)=>boolean=(token)=>token.kind==='is'||token.kind==='tag'){
  const parts=inlineSearchParts(input,tokens).filter(part=>part.kind==='text'||include(part.token));let result='';
  for(let index=0;index<parts.length;index+=1){const part=parts[index],value=part.kind==='text'?part.value:part.token.raw;if(!value)continue;const previous=result.at(-1),next=parts.at(index+1),nextValue=next?.kind==='text'?next.value:next?.token.raw;if(part.kind==='token'&&previous&&!/[\s(]/.test(previous))result+=' ';result+=value;if(part.kind==='token'&&nextValue&&!/^[\s)]/.test(nextValue))result+=' '}
  return result.trim();
}

/** Treat a complete token still in the editor as structured search without changing the UI. */
export function effectiveSearch(input:string,tokens:readonly InlineSearchToken[]){
  const parsed=consumeSearchToken(input,true),text=parsed.token?parsed.text:input,next=parsed.token&&!tokens.some(token=>token.kind===parsed.token!.kind&&token.value===parsed.token!.value)?[...tokens,{...parsed.token,offset:text.length}]:[...tokens];
  const ordered=orderedSearchText(text,next),advanced=/(?:^|[\s(])(?:AND|OR|NOT)(?=$|[\s)])|[()]/i.test(ordered);
  return{text:advanced?ordered:orderedSearchText(text,next,token=>token.kind==='is'),tokens:next};
}

export function tokenQuery(tokens:readonly InlineSearchToken[]){
  const query:Record<string,string|boolean>={};
  const tags=tokens.filter((token):token is Extract<InlineSearchToken,{kind:'tag'}>=>token.kind==='tag').map(token=>token.value);
  if(tags.length)query.tags=tags.join(',');
  if(tokens.some(token=>token.kind==='has'&&token.value==='attachment'))query.has_attachment=true;
  if(tokens.some(token=>token.kind==='has'&&token.value==='media-annotation'))query.has_media_annotation=true;
  if(tokens.some(token=>token.kind==='has'&&token.value==='commit'))query.has_commit=true;
  const attachment=[...tokens].reverse().find((token):token is Extract<InlineSearchToken,{kind:'attachment'}>=>token.kind==='attachment');
  if(attachment)query.attachment=attachment.value;
  for(const token of tokens){if(token.kind!=='date')continue;const field=token.field==='started'||token.field==='archived'?'updated':token.field;query[`${field}_${token.direction}`]=token.value;if(token.field==='started')query.status='started';if(token.field==='archived')query.status='archive'}
  return query;
}
