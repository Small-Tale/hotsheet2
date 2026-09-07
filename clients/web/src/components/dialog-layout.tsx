import './dialog-layout.css';

import type { SafeHtml } from 'kerfjs/jsx-runtime';

export interface DialogHeaderProps {
  title: string;
  titleId: string;
  summary: string;
  summaryId?: string;
  icon?: SafeHtml;
  iconClassName?: string;
  actions?: SafeHtml;
}

export function DialogHeader({title,titleId,summary,summaryId,icon,iconClassName='',actions}:DialogHeaderProps){
  return <header class="dialog-header" data-component="dialog-header" data-has-icon={String(Boolean(icon))}>
    {icon&&<span class={`dialog-header__icon ${iconClassName}`.trim()}>{icon}</span>}
    <div class="dialog-header__copy"><h2 id={titleId}>{title}</h2><p id={summaryId}>{summary}</p></div>
    {actions&&<div class="dialog-header__actions">{actions}</div>}
  </header>;
}

export function ValueTable({label,className='',children}:{label:string;className?:string;children:SafeHtml|SafeHtml[]}){
  return <dl class={`value-table ${className}`.trim()} data-component="value-table" aria-label={label}>{children}</dl>;
}
