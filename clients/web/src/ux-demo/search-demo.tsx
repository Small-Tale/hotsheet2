import {GlobalSearchOverlay} from '../components/global-search-overlay';
const results=[
  {connection_id:'local',native_id:'HS2-383D6K',qualified_id:'local:HS2-383D6K',id:'one',slug:'HS2-383D6K',title:'Complete advanced web search and active-filter UX',status:'started',up_next:true,feedback_needed:false,tags:['web','search'],blocked_by:[],claim_count:1,matchLabel:'Exact ticket',providerLabel:'Local Hot Sheet'},
  {connection_id:'github',native_id:'418',qualified_id:'github:418',id:'two',slug:'HS2-INDEX7',title:'Index ticket references in notes',status:'not_started',up_next:false,feedback_needed:true,tags:['index'],blocked_by:[],claim_count:0,matchLabel:'Ticket reference',providerLabel:'GitHub'},
];
export function GlobalSearchDemo(){return <section class="global-search-demo"><GlobalSearchOverlay open query="HS2-383D6K" scope="working" filters={['up-next','needs-review']} results={results} embedded/></section>}
