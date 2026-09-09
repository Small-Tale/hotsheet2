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

function localizedDigits(locale?:string){const digits=new Map<string,string>();for(let value=0;value<10;value++)digits.set(new Intl.NumberFormat(locale,{useGrouping:false}).format(value),String(value));return digits}
function latinNumber(value:string,locale?:string){const digits=localizedDigits(locale);return Number(Array.from(value,character=>digits.get(character)??character).join(''))}
function localDateOrder(locale?:string){return new Intl.DateTimeFormat(locale,{year:'numeric',month:'numeric',day:'numeric'}).formatToParts(new Date(2006,10,22)).filter(part=>part.type==='year'||part.type==='month'||part.type==='day').map(part=>part.type as 'year'|'month'|'day')}
function localDayPeriods(locale?:string){const value=(hour:number)=>new Intl.DateTimeFormat(locale,{hour:'numeric',hour12:true}).formatToParts(new Date(2000,0,1,hour)).find(part=>part.type==='dayPeriod')?.value.toLocaleLowerCase(locale);return{am:value(1),pm:value(13)}}

function validLocalDate(year:number,month:number,day:number,hour:number,minute:number){
  if(month<1||month>12||day<1||day>31||hour<0||hour>23||minute<0||minute>59)return undefined;
  const date=new Date(year,month-1,day,hour,minute);
  return date.getFullYear()===year&&date.getMonth()===month-1&&date.getDate()===day?date:undefined;
}

export function parseSearchDate(value:string,locale?:string):string|undefined{
  const source=value.trim(),iso=source.match(/^(\d{4})-(\d{2})-(\d{2})(?:[T ](\d{2}):(\d{2})(?::\d{2}(?:\.\d+)?)?(Z|[+-]\d{2}:?\d{2})?)?$/i);
  if(iso){
    if(iso[6]){if(!validLocalDate(Number(iso[1]),Number(iso[2]),Number(iso[3]),12,0)||Number(iso[4])>23||Number(iso[5])>59)return undefined;const timestamp=Date.parse(source);return Number.isNaN(timestamp)?undefined:new Date(timestamp).toISOString()}
    return validLocalDate(Number(iso[1]),Number(iso[2]),Number(iso[3]),Number(iso[4]||0),Number(iso[5]||0))?.toISOString();
  }
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
