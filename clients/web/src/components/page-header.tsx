import './page-header.css';

export function PageHeader({ title }: { title: string }) {
  return <header class="page-header" data-component="page-header"><h1>{title}</h1></header>;
}
