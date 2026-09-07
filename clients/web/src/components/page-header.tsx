import './page-header.css';

import type { SafeHtml } from 'kerfjs/jsx-runtime';

export function PageHeader({ title, action }: { title: string; action?: SafeHtml }) {
  return <header class="page-header" data-component="page-header"><h1>{title}</h1>{action&&<div class="page-header__action">{action}</div>}</header>;
}
