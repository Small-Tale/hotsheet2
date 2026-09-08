import {signal} from 'kerfjs';

import { AttachmentContextMenu } from '../components/attachment-context-menu';
import { AttachmentGallery,attachmentGalleryZoomModel } from '../components/attachment-gallery';
import { TicketAttachments } from '../components/ticket-attachments';
import { TicketCategorySelect } from '../components/ticket-category-select';
import { TicketCodeReview } from '../components/ticket-code-review';
import { TicketInfoPanel } from '../components/ticket-info-panel';
import { TicketPrioritySelect } from '../components/ticket-priority-select';
import { TicketStatusMenu } from '../components/ticket-status-menu';
import { TicketTimeline } from '../components/ticket-timeline';

export function TicketCategorySelectDemo() { return <section class="metadata-control-demo" aria-label="TicketCategorySelect demo"><TicketCategorySelect name="demo-category" value="feature" /></section>; }
export function TicketPrioritySelectDemo() { return <section class="metadata-control-demo" aria-label="TicketPrioritySelect demo"><TicketPrioritySelect name="demo-priority" value="urgent" /></section>; }
export function TicketStatusMenuDemo() { return <section class="metadata-control-demo" aria-label="TicketStatusMenu demo"><div><span>Status</span><TicketStatusMenu value="started" /></div></section>; }
export function TicketInfoPanelDemo() { return <section class="inspector-panel-demo" aria-label="TicketInfoPanel demo"><TicketInfoPanel status="started" priority="high" category="feature" tags={['client', 'ux']} details={'## Implementation notes\n\nBuild the reusable metadata and details presentation independently from the inspector shell.'} blockedReason="Waiting for final design review." notes={[{ id: 'review', kind: 'regular', author: 'Claude', time: '10 minutes ago', body: 'The metadata and notes now share the inspector’s controlled state.' }]} providerName="Hot Sheet git" updatedLabel="Updated 2 minutes ago" /></section>; }
export function TicketTimelineDemo() { return <section class="inspector-panel-demo" aria-label="TicketTimeline demo"><TicketTimeline /></section>; }
const demoImages=[{id:'wide',name:'wide-layout.svg',url:'/ux-gallery-preview.svg'},{id:'narrow',name:'narrow-layout.svg',url:'/ux-gallery-preview.svg?variant=narrow'},{id:'video',name:'walkthrough.mp4',url:'/ux-gallery-video.mp4'}];
export const galleryDemoUrl=signal<string|undefined>(demoImages[0].url);
export const galleryDemoScale=signal<number|undefined>(undefined);
const galleryDemoGeometry={naturalWidth:1600,naturalHeight:1000,availableWidth:1100,availableHeight:650};
export function setGalleryDemo(open:boolean){galleryDemoUrl.value=open?demoImages[0].url:undefined;galleryDemoScale.value=undefined}
export function shiftGalleryDemo(delta:number){const index=demoImages.findIndex(image=>image.url===galleryDemoUrl.value);galleryDemoUrl.value=demoImages[(Math.max(0,index)+delta+demoImages.length)%demoImages.length].url;galleryDemoScale.value=undefined}
export function zoomGalleryDemo(direction:'in'|'out'){const model=attachmentGalleryZoomModel(galleryDemoGeometry,galleryDemoScale.value);galleryDemoScale.value=model.stops[model.index+(direction==='in'?1:-1)]}
export const attachmentDemoMenu=signal<{x:number;y:number}|undefined>(undefined);
export function showAttachmentDemoMenu(x:number,y:number){attachmentDemoMenu.value={x,y}}
export function closeAttachmentDemoMenu(){attachmentDemoMenu.value=undefined}
export function TicketAttachmentsDemo() { const menu=attachmentDemoMenu.value;return <section class="inspector-panel-demo" aria-label="TicketAttachments demo"><TicketAttachments attachments={[...demoImages,{ id: 'demo-video', name: 'choppy.mov',url:'/ux-gallery-preview.svg?variant=video' }]} />{menu&&<AttachmentContextMenu x={menu.x} y={menu.y}/>}</section>; }
export function AttachmentGalleryDemo(){return galleryDemoUrl.value?<AttachmentGallery images={demoImages} activeUrl={galleryDemoUrl.value} geometry={galleryDemoGeometry} selectedScale={galleryDemoScale.value}/>:<button type="button" data-action="open-gallery-demo">Open gallery</button>}
export function TicketCodeReviewDemo() { return <section class="code-review-demo" aria-label="TicketCodeReview demo"><article><h3>Configured — disjoint bundles</h3><div class="inspector-panel-demo"><TicketCodeReview review={{difftool:'Glassbox',truncated:false,summary:{files:{total:9,docs:2,tests:3,source:3,other:1},tests_added:2,tests_modified:1},ranges:[{from:'aaa1111',to:'bbb2222',count:2},{from:'ccc3333',to:'ddd4444',count:2}],commits:[{sha:'ddd4444',short_sha:'ddd4444',subject:'HS2-DEMO: complete the later review bundle',committed_at:'2026-09-02T10:00:00Z'},{sha:'ccc3333',short_sha:'ccc3333',subject:'HS2-DEMO: begin the later review bundle',committed_at:'2026-09-02T09:00:00Z'},{sha:'bbb2222',short_sha:'bbb2222',subject:'HS2-DEMO: complete the initial review bundle',committed_at:'2026-09-02T08:00:00Z'},{sha:'aaa1111',short_sha:'aaa1111',subject:'HS2-DEMO: begin the initial review bundle',committed_at:'2026-09-02T07:00:00Z'}]}}/></div></article><article><h3>Not configured</h3><div class="inspector-panel-demo"><TicketCodeReview review={{truncated:false,ranges:[],commits:[{sha:'aaa',short_sha:'aaaaaaa',subject:'HS2-DEMO: readable without a tool',committed_at:'2026-09-02T07:00:00Z'}]}}/></div></article><article><h3>Empty</h3><div class="inspector-panel-demo"><TicketCodeReview review={{difftool:'Meld',truncated:false,ranges:[],commits:[]}}/></div></article><article><h3>Loading / error</h3><div class="inspector-panel-demo"><TicketCodeReview loading message="The repository is temporarily unavailable."/></div></article></section>; }
