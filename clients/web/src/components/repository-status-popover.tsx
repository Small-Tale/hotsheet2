import './repository-status-popover.css';

import { ArrowDown, ArrowUp, CircleCheck, Clipboard, Copy, ExternalLink, FolderOpen, GitBranch, GitCommitHorizontal, RefreshCw, SquareMinus, SquarePen, SquarePlus, SquareX, TriangleAlert } from 'lucide';

import type { CodeReview, RepositoryFile, RepositoryFileChange, RepositoryStatus } from '../api';
import { LucideIcon } from './lucide-icon';
import { MenuHeader } from './menu-header';
import { MenuItem } from './menu-item';
import { type CodeReviewComparison, TicketCodeReview } from './ticket-code-review';

export type RepositoryStatusState='clean'|'dirty'|'ahead'|'behind'|'diverged'|'conflicted'|'error';
export type RepositoryStatusView='staged'|'unstaged'|'untracked'|'conflicted'|'commits';
export interface RepositoryFileMenu {path:string;absolutePath?:string;x:number;y:number}

export function repositoryStatusState(status:RepositoryStatus|null,error=''):RepositoryStatusState {
  if(error||!status)return 'error';
  if(status.conflicted>0)return 'conflicted';
  if(status.ahead>0&&status.behind>0)return 'diverged';
  if(status.behind>0)return 'behind';
  if(status.ahead>0)return 'ahead';
  if(status.staged+status.unstaged+status.untracked>0)return 'dirty';
  return 'clean';
}

const stateCopy:Record<RepositoryStatusState,string>={
  clean:'Working tree is clean',dirty:'Local changes have not been committed',ahead:'Local commits have not been pushed',behind:'Remote commits have not been integrated',diverged:'Local and remote histories have diverged',conflicted:'Repository has unresolved conflicts',error:'Repository status is unavailable',
};

const viewDefinitions=[
  {id:'staged',label:'Staged',icon:SquarePlus},
  {id:'unstaged',label:'Unstaged',icon:SquareMinus},
  {id:'untracked',label:'Untracked',icon:SquarePen},
  {id:'conflicted',label:'Conflicted',icon:SquareX},
  {id:'commits',label:'Commits',icon:GitCommitHorizontal},
] as const;

export function RepositoryStatusPopover({status,error='',refreshing=false,view='unstaged',fileMenu,embedded=false,comparison,expandedCommits,detailFiles,detailCommits,detailLoading=false,detailError='',detailHasMore=false}:{status:RepositoryStatus|null;error?:string;refreshing?:boolean;view?:RepositoryStatusView;fileMenu?:RepositoryFileMenu;embedded?:boolean;comparison?:CodeReviewComparison;expandedCommits?:readonly string[];detailFiles?:RepositoryFile[];detailCommits?:CodeReview['commits'];detailLoading?:boolean;detailError?:string;detailHasMore?:boolean}){
  const state=repositoryStatusState(status,error),branch=status?.branch||'No branch',upstream=status?.upstream||'No upstream';
  const files=repositoryFilesForView(detailFiles??status?.files??[],view);
  const review:CodeReview|undefined=status?{commits:detailCommits??status.commits??[],ranges:status.ranges??[],difftool:status.difftool,truncated:Boolean(status.truncated)}:undefined;
  return <section popover={embedded?undefined:'auto'} id={embedded?undefined:'repository-status-popover'} class="repository-status-popover" data-component="repository-status-popover" data-state={state} data-view={view} data-embedded={embedded?'true':undefined} role="dialog" aria-labelledby="repository-status-title">
    <header><span class="repository-status-popover__icon"><LucideIcon icon={state==='clean'?CircleCheck:state==='error'||state==='conflicted'?TriangleAlert:GitBranch} name={state==='clean'?'circle-check':state==='error'||state==='conflicted'?'triangle-alert':'git-branch'}/></span><div><h2 id="repository-status-title">Repository Status</h2><p>{stateCopy[state]}</p></div><button type="button" class="repository-status-popover__refresh" data-action="refresh-repository-status" disabled={refreshing} aria-label={refreshing?'Refreshing repository status':'Refresh repository status'}><LucideIcon icon={RefreshCw} name="refresh-cw"/></button></header>
    {status&&<div class="repository-status-popover__layout"><aside>
      <dl class="repository-status-popover__values"><div><dt>Branch</dt><dd>{branch}</dd></div><div><dt>Upstream</dt><dd>{upstream}</dd></div></dl>
      <dl class="repository-status-popover__values"><div><dt>Ahead</dt><dd><LucideIcon icon={ArrowUp} name="arrow-up"/>{status.ahead}</dd></div><div><dt>Behind</dt><dd><LucideIcon icon={ArrowDown} name="arrow-down"/>{status.behind}</dd></div></dl>
      <nav aria-label="Repository views"><MenuHeader label="Views"/>{viewDefinitions.map(item=><MenuItem action="select-repository-view" itemId={item.id} selected={view===item.id} icon={<LucideIcon icon={item.icon} name={item.id==='commits'?'git-commit-horizontal':item.id==='untracked'?'square-pen':item.id==='conflicted'?'square-x':item.id==='staged'?'square-plus':'square-minus'}/>} label={item.label} trailing={<small class="menu-item__count">{repositoryViewCount(status,item.id)}</small>}/>)}</nav>
    </aside><main class="repository-status-popover__detail" aria-live="polite">
      {view==='commits'?<TicketCodeReview embedded title="Commits" emptyMessage="No commits were found in this repository." loadingMessage="Finding commits…" action="open-repository-review" review={review} comparison={comparison} expandedCommits={expandedCommits} loading={detailLoading&&review?.commits.length===0}/>:<RepositoryFileList files={files} view={view} loading={detailLoading}/>}
      {detailError&&<p class="repository-status-popover__detail-error" role="alert">{detailError}</p>}
      {(detailHasMore||detailLoading)&&<div class="repository-status-popover__pagination" data-repository-pagination-sentinel="true" role="status">{detailLoading?'Loading more…':'Load more'}</div>}
    </main></div>}
    {!status&&!error&&<p class="repository-status-popover__loading" role="status">Loading repository status…</p>}
    {error&&<p class="repository-status-popover__error" role="alert">{error}</p>}
    {fileMenu&&<RepositoryFileContextMenu menu={fileMenu} platform={status?.platform}/>}
  </section>;
}

function RepositoryFileList({files,view,loading=false}:{files:RepositoryFile[];view:Exclude<RepositoryStatusView,'commits'>;loading?:boolean}){
  if(files.length===0&&loading)return <></>;
  if(files.length===0)return <div class="repository-status-popover__empty"><LucideIcon icon={CircleCheck} name="circle-check"/><p>No {view} files.</p></div>;
  return <div class="repository-status-popover__files" role="list" aria-label={`${view} files`}>{files.map(file=>{
    const change=repositoryFileChange(file,view);
    return <MenuItem action="open-repository-file" itemId={file.path} className="repository-status-popover__file" state={change} multiline title={`${file.path} — ${fileChangeLabel(change)}. Double-click to open; right-click for more actions`} accessibleLabel={`${file.path}, ${fileChangeLabel(change)}`} icon={<span class="repository-status-popover__file-status" aria-hidden="true">{repositoryFileStatusLetter(change)}</span>} label={<>{middleEllipsisPath(file.path)}{file.original_path&&<small>from {middleEllipsisPath(file.original_path)}</small>}</>}/>;
  })}</div>;
}

function RepositoryFileContextMenu({menu,platform}:{menu:RepositoryFileMenu;platform?:RepositoryStatus['platform']}){
  const reveal=platform==='macos'?'Show in Finder':platform==='windows'?'Show in File Explorer':'Show in file manager';
  return <div class="repository-status-popover__context-menu" data-component="repository-file-context-menu" role="menu" style={`left:${menu.x}px;top:${menu.y}px`}>
    <button type="button" role="menuitem" data-repository-file-action="open" data-repository-file-path={menu.path}><LucideIcon icon={ExternalLink} name="external-link"/>Open</button>
    <button type="button" role="menuitem" data-repository-file-action="copy-path" data-repository-file-path={menu.path}><LucideIcon icon={Clipboard} name="clipboard"/>Copy path</button>
    {menu.absolutePath&&<button type="button" role="menuitem" data-repository-file-action="copy-absolute-path" data-repository-file-path={menu.absolutePath}><LucideIcon icon={Copy} name="copy"/>Copy absolute path</button>}
    <hr/>
    <button type="button" role="menuitem" data-repository-file-action="reveal" data-repository-file-path={menu.path}><LucideIcon icon={FolderOpen} name="folder-open"/>{reveal}</button>
  </div>;
}

export function repositoryFilesForView(files:RepositoryFile[],view:RepositoryStatusView):RepositoryFile[]{
  if(view==='commits')return [];
  return files.filter(file=>view==='staged'?Boolean(file.staged)&&!file.conflicted:view==='unstaged'?Boolean(file.unstaged)&&!file.untracked&&!file.conflicted:view==='untracked'?file.untracked:file.conflicted);
}

export function repositoryAbsolutePath(root:string|undefined,path:string,platform:RepositoryStatus['platform']):string|undefined{
  if(!root)return undefined;
  const separator=platform==='windows'?'\\':'/';
  return `${root.replace(/[\\/]+$/,'')}${separator}${platform==='windows'?path.replaceAll('/','\\'):path}`;
}

function repositoryViewCount(status:RepositoryStatus,view:RepositoryStatusView):number{
  if(view==='commits')return status.commit_count??status.commits?.length??0;
  return status[view];
}

function repositoryFileChange(file:RepositoryFile,view:Exclude<RepositoryStatusView,'commits'>):RepositoryFileChange{
  if(view==='conflicted')return 'unmerged';
  if(view==='untracked')return 'untracked';
  return file[view]??'modified';
}

export function repositoryFileStatusLetter(change:RepositoryFileChange):string{
  return change==='untracked'?'?':change==='type_changed'?'T':change==='unmerged'?'U':change[0].toUpperCase();
}

function fileChangeLabel(change:RepositoryFileChange):string{
  return change==='type_changed'?'Type changed':change[0].toUpperCase()+change.slice(1);
}

function middleEllipsisPath(path:string){
  const suffixLength=Math.min(20,Math.floor(path.length/2)),splitAt=path.length-suffixLength;
  return <span class="repository-status-popover__path" aria-label={path}><span>{path.slice(0,splitAt)}</span><span>{path.slice(splitAt)}</span></span>;
}
