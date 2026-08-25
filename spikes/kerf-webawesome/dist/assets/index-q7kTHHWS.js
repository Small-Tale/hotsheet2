(function(){const e=document.createElement("link").relList;if(e&&e.supports&&e.supports("modulepreload"))return;for(const o of document.querySelectorAll('link[rel="modulepreload"]'))i(o);new MutationObserver(o=>{for(const r of o)if(r.type==="childList")for(const a of r.addedNodes)a.tagName==="LINK"&&a.rel==="modulepreload"&&i(a)}).observe(document,{childList:!0,subtree:!0});function n(o){const r={};return o.integrity&&(r.integrity=o.integrity),o.referrerPolicy&&(r.referrerPolicy=o.referrerPolicy),o.crossOrigin==="use-credentials"?r.credentials="include":o.crossOrigin==="anonymous"?r.credentials="omit":r.credentials="same-origin",r}function i(o){if(o.ep)return;o.ep=!0;const r=n(o);fetch(o.href,r)}})();var L={},mi=Symbol.for("preact-signals");function ue(){if(W>1)W--;else{var t,e=!1;for((function(){var o=Ft;for(Ft=void 0;o!==void 0;){var r=o.S;if(r.v===o.v)for(var a=r.t;a!==void 0;a=a.x)a.i===o.i&&(a.i=r.i);o=o.o}})();lt!==void 0;){var n=lt;for(lt=void 0,Tt++;n!==void 0;){var i=n.u;if(n.u=void 0,n.f&=-3,!(8&n.f)&&bn(n))try{n.c()}catch(o){e||(t=o,e=!0)}n=i}}if(Tt=0,W--,e)throw t}}var st,b=void 0;function Pt(t){var e=b,n=st;b=void 0,st=void 0;try{return t()}finally{b=e,st=n}}var lt=void 0,W=0,Tt=0,Oe=0,Ft=void 0,It=0;function wn(t){if(b!==void 0){var e=t.n;if(e===void 0||e.t!==b)return e={i:0,S:t,p:b.s,n:void 0,t:b,e:void 0,x:void 0,r:e},b.s!==void 0&&(b.s.n=e),b.s=e,t.n=e,32&b.f&&t.S(e),e;if(e.i===-1)return e.i=0,e.n!==void 0&&(e.n.p=e.p,e.p!==void 0&&(e.p.n=e.n),e.p=b.s,e.n=void 0,b.s.n=e,b.s=e),e}}function k(t,e){this.v=t,this.i=0,this.n=void 0,this.t=void 0,this.l=0,this.W=e?.watched,this.Z=e?.unwatched,this.name=e?.name}k.prototype.brand=mi;k.prototype.h=function(){return!0};k.prototype.S=function(t){var e=this,n=this.t;n!==t&&t.e===void 0&&(t.x=n,this.t=t,n!==void 0?n.e=t:Pt(function(){var i;(i=e.W)==null||i.call(e)}))};k.prototype.U=function(t){var e=this;if(this.t!==void 0){var n=t.e,i=t.x;n!==void 0&&(n.x=i,t.e=void 0),i!==void 0&&(i.e=n,t.x=void 0),t===this.t&&(this.t=i,i===void 0&&Pt(function(){var o;(o=e.Z)==null||o.call(e)}))}};k.prototype.subscribe=function(t){var e=this;return xn(function(){var n=e.value;Pt(function(){return t(n)})},{name:"sub"})};k.prototype.valueOf=function(){return this.value};k.prototype.toString=function(){return this.value+""};k.prototype.toJSON=function(){return this.value};k.prototype.peek=function(){var t=this;return Pt(function(){return t.value})};Object.defineProperty(k.prototype,"value",{get:function(){var t=wn(this);return t!==void 0&&(t.i=this.i),this.v},set:function(t){if(t!==this.v){if(Tt>100)throw new Error("Cycle detected");(function(n){W!==0&&Tt===0&&n.l!==Oe&&(n.l=Oe,Ft={S:n,v:n.v,i:n.i,o:Ft})})(this),this.v=t,this.i++,It++,W++;try{for(var e=this.t;e!==void 0;e=e.x)e.t.N()}finally{ue()}}}});function pi(t,e){return new k(t,e)}function bn(t){for(var e=t.s;e!==void 0;e=e.n)if(e.S.i!==e.i||!e.S.h()||e.S.i!==e.i)return!0;return!1}function yn(t){for(var e=t.s;e!==void 0;e=e.n){var n=e.S.n;if(n!==void 0&&(e.r=n),e.S.n=e,e.i=-1,e.n===void 0){t.s=e;break}}}function Cn(t){for(var e=t.s,n=void 0;e!==void 0;){var i=e.p;e.i===-1?(e.S.U(e),i!==void 0&&(i.n=e.n),e.n!==void 0&&(e.n.p=i)):n=e,e.S.n=e.r,e.r!==void 0&&(e.r=void 0),e=i}t.s=n}function et(t,e){k.call(this,void 0,e),this.x=t,this.s=void 0,this.g=It-1,this.f=4}et.prototype=new k;et.prototype.h=function(){if(this.f&=-3,1&this.f)return!1;if((36&this.f)==32||(this.f&=-5,this.g===It))return!0;if(this.g=It,this.f|=1,this.i>0&&!bn(this))return this.f&=-2,!0;var t=b;try{yn(this),b=this;var e=this.x();(16&this.f||this.v!==e||this.i===0)&&(this.v=e,this.f&=-17,this.i++)}catch(n){this.v=n,this.f|=16,this.i++}return b=t,Cn(this),this.f&=-2,!0};et.prototype.S=function(t){if(this.t===void 0){this.f|=36;for(var e=this.s;e!==void 0;e=e.n)e.S.S(e)}k.prototype.S.call(this,t)};et.prototype.U=function(t){if(this.t!==void 0&&(k.prototype.U.call(this,t),this.t===void 0)){this.f&=-33;for(var e=this.s;e!==void 0;e=e.n)e.S.U(e)}};et.prototype.N=function(){if(!(2&this.f)){this.f|=6;for(var t=this.t;t!==void 0;t=t.x)t.t.N()}};Object.defineProperty(et.prototype,"value",{get:function(){if(1&this.f)throw new Error("Cycle detected");var t=wn(this);if(this.h(),t!==void 0&&(t.i=this.i),16&this.f)throw this.v;return this.v}});function Ln(t){var e=t.m;if(t.m=void 0,typeof e=="function"){W++;var n=b;b=void 0;try{e()}catch(i){throw t.f&=-2,t.f|=8,he(t),i}finally{b=n,ue()}}}function he(t){for(var e=t.s;e!==void 0;e=e.n)e.S.U(e);t.x=void 0,t.s=void 0,Ln(t)}function gi(t){if(b!==this)throw new Error("Out-of-order effect");Cn(this),b=t,this.f&=-2,8&this.f&&he(this),ue()}function nt(t,e){this.x=t,this.m=void 0,this.s=void 0,this.u=void 0,this.f=32,this.name=e?.name,st&&st.push(this)}nt.prototype.c=function(){var t=this.S();try{if(8&this.f||this.x===void 0)return;var e=this.x();typeof e=="function"&&(this.m=e)}finally{t()}};nt.prototype.S=function(){if(1&this.f)throw new Error("Cycle detected");this.f|=1,this.f&=-9,Ln(this),yn(this),W++;var t=b;return b=this,gi.bind(this,t)};nt.prototype.N=function(){2&this.f||(this.f|=2,this.u=lt,lt=this)};nt.prototype.d=function(){this.f|=8,1&this.f||he(this)};nt.prototype.dispose=function(){this.d()};function xn(t,e){var n=new nt(t,e);try{n.c()}catch(o){throw n.d(),o}var i=n.d.bind(n);return i[Symbol.dispose]=i,i}function Sn(t){return t instanceof k}function fe(t){const e=L.signalFactory;return e?e(t):pi(t)}function me(t){const e=L.wrapEffect;return xn(e?e(t):t)}var K="kf-list:";function pe(t,e){if(t.kind==="static")return t.html;if(t.kind==="list"){const n=t.items.map(i=>i.html).join("");return e?`<!--${K}${t.id}-->${n}`:n}return t.parts.map(n=>pe(n,e)).join("")}function ge(t){return t.kind==="static"?t.html:t.kind==="list"?`<!--${K}${t.id}-->`:t.parts.map(ge).join("")}function vt(t,e=new Map){if(t.kind==="list")e.set(t.id,t);else if(t.kind==="mixed")for(const n of t.parts)vt(n,e);return e}function vi(t){if(t.length===0)return{kind:"static",html:""};if(t.every(i=>i.kind==="static"))return{kind:"static",html:t.map(i=>i.html).join("")};const e=[];let n="";for(const i of t)i.kind==="static"?n+=i.html:(n!==""&&(e.push({kind:"static",html:n}),n=""),e.push(i));return n!==""&&e.push({kind:"static",html:n}),{kind:"mixed",parts:e}}function wi(t,e,n){return t.kind==="static"?{kind:"static",html:e+t.html+n}:t.kind==="mixed"?{kind:"mixed",parts:[{kind:"static",html:e},...t.parts,{kind:"static",html:n}]}:{kind:"mixed",parts:[{kind:"static",html:e},t,{kind:"static",html:n}]}}function P(t,e,n,i){const o=t.tagName;e==="checked"?o==="INPUT"&&(t.checked=i):e==="value"?o==="INPUT"&&t!==document.activeElement&&(t.value=i?n:""):e==="selected"&&o==="OPTION"&&(t.selected=i)}var bi=new Set(["href","src","xlink:href","formaction","action","data"]),yi=new Set(["javascript","vbscript"]),Ci=new Set(["javascript:","javascript:;","javascript:void(0)","javascript:void(0);","javascript:void 0","javascript:void 0;"]),Li=/[\u0000-\u001F\u007F]/g;function xi(t){return t.replace(Li,"").replace(/^\s+/,"")}function Si(t){const e=/^([a-zA-Z][a-zA-Z0-9+.-]*):/.exec(t);return e?e[1].toLowerCase():null}function $i(t){const e=/^data:([^;,]*)/.exec(t.toLowerCase())?.[1].trim()??"";return e===""||e==="text/plain"||e==="text/css"?!1:e==="image/svg+xml"?!0:!(e.startsWith("image/")||e.startsWith("font/")||e.startsWith("application/font")||e.startsWith("audio/")||e.startsWith("video/"))}function $n(t,e){if(!bi.has(t))return!1;const n=xi(e),i=Si(n);return i===null?!1:yi.has(i)?!Ci.has(n.trimEnd().toLowerCase()):i==="data"&&$i(n)}function ki(t,e){return`dropped dangerous URL value for ${t}=${JSON.stringify(e.slice(0,80))}. kerf blocks javascript:, vbscript:, and script-executing data: URLs (text/html, image/svg+xml, xml) in href/src/data/formaction/action/xlink:href by default. Wrap in raw() if this is intentional (e.g. bookmarklets), or sanitize upstream.`}function kn(t,e,n){const i=`${t}: ${ki(e,n)}`;L.urlScreenThrow?.(i),console.warn(i)}var An="data-kfb",ve="kfb:",De="data-kfbrow",zn="kfbr:",H=null,Ai=Object.freeze([]);function zi(){return{counter:0,list:[]}}function Be(t){H=t}function Ei(t,e){if(H!==null){const n=`a${H.counter++}`;return H.list.push({kind:"attr",id:n,attr:t,signal:e}),n}return null}function _i(){return An}function Mi(t){if(H!==null){const e=`t${H.counter++}`;return H.list.push({kind:"text",id:e,signal:t}),`<!--${ve}${e}-->`}return null}function Pe(t,e,n){for(const o of n)o();if(e.list.length===0)return Ai;const i=[];return Ti(t,e.list,i),i}function Ut(t,e){const n=new Array(e.length),i=t.getAttribute(De);let o=null,r=null,a=null;for(let s=0;s<e.length;s++){const l=e[s];if(l.kind==="attr"){let c=!1;i!==null&&(i===l.id?c=!0:i.indexOf(",")!==-1&&(o??=new Set(i.split(",")),c=o.has(l.id)));let u;if(c?u=t:(r??=En(t,De),u=r.get(l.id)),u===void 0)continue;n[s]=_n(u,l.attr,l.signal)}else{a===null&&(a=new Map,we(t,zn,a));const c=a.get(l.id);if(c===void 0)continue;n[s]=Mn(c,l.signal)}}return n}function U(t){if(t!==void 0)for(const e of t)e()}function zt(t,e,n,i){const o=e===void 0?0:e.length,r=i===void 0?0:i.length;if(o===r){let a=!0;for(let s=0;s<r;s++)if(e[s].signal!==i[s].signal){a=!1;break}if(a)return{bindings:e,bindingDisposers:n}}return U(n),{bindings:i,bindingDisposers:r>0?Ut(t,i):void 0}}function Ti(t,e,n){const i=En(t,An),o=new Map;we(t,ve,o);for(const r of e)if(r.kind==="attr"){const a=i.get(r.id);if(a===void 0)continue;n.push(_n(a,r.attr,r.signal))}else{const a=o.get(r.id);if(a===void 0)continue;n.push(Mn(a,r.signal))}}function En(t,e){const n=new Map;for(const i of t.querySelectorAll(`[${e}]`))for(const o of i.getAttribute(e).split(","))n.set(o,i);return n}function _n(t,e,n){return me(()=>Ii(t,e,n.value))}var ne=new WeakMap;function Fi(t){const e=ne.get(t);return e!==void 0&&t.nextSibling===e?e:null}function Mn(t,e){let n=ne.get(t);(n===void 0||t.nextSibling!==n)&&(n=t.ownerDocument.createTextNode(""),t.parentNode.insertBefore(n,t.nextSibling),ne.set(t,n));const i=n;return me(()=>{i.data=Oi(e.value)})}function Ii(t,e,n){if(n==null||n===!1){t.removeAttribute(e),P(t,e,"",!1);return}if(n===!0){t.setAttribute(e,""),P(t,e,"",!0);return}if(Ni(n)){t.setAttribute(e,n.__html),P(t,e,n.__html,!0);return}const i=String(n);if($n(e,i)){kn("kerf binding",e,i),t.removeAttribute(e);return}t.setAttribute(e,i),P(t,e,i,!0)}var Ri=Symbol.for("kerfjs.SafeHtml");function Ni(t){return typeof t=="object"&&t!==null&&t[Ri]===!0}function Oi(t){return t==null||typeof t=="boolean"?"":String(t)}function we(t,e,n){for(let i=t.firstChild;i!==null;i=i.nextSibling)if(i.nodeType===Node.COMMENT_NODE){const o=i.data;o.startsWith(e)&&n.set(o.slice(e.length),i)}else i.nodeType===Node.ELEMENT_NODE&&we(i,e,n)}function Ue(t){return t.replace(/&/g,"&amp;").replace(/</g,"&lt;").replace(/>/g,"&gt;").replace(/"/g,"&quot;")}function Di(t){return t.replace(/&/g,"&amp;").replace(/"/g,"&quot;").replace(/'/g,"&#39;").replace(/</g,"&lt;").replace(/>/g,"&gt;")}var Tn={className:"class",htmlFor:"for",httpEquiv:"http-equiv",acceptCharset:"accept-charset",accessKey:"accesskey",autoCapitalize:"autocapitalize",autoComplete:"autocomplete",autoFocus:"autofocus",autoPlay:"autoplay",colSpan:"colspan",contentEditable:"contenteditable",crossOrigin:"crossorigin",dateTime:"datetime",defaultChecked:"checked",defaultSelected:"selected",defaultValue:"value",encType:"enctype",formAction:"formaction",formEncType:"formenctype",formMethod:"formmethod",formNoValidate:"formnovalidate",formTarget:"formtarget",hrefLang:"hreflang",inputMode:"inputmode",maxLength:"maxlength",minLength:"minlength",noModule:"nomodule",noValidate:"novalidate",readOnly:"readonly",referrerPolicy:"referrerpolicy",rowSpan:"rowspan",spellCheck:"spellcheck",srcDoc:"srcdoc",srcLang:"srclang",srcSet:"srcset",tabIndex:"tabindex",useMap:"usemap",strokeWidth:"stroke-width",strokeLinecap:"stroke-linecap",strokeLinejoin:"stroke-linejoin",strokeDasharray:"stroke-dasharray",strokeDashoffset:"stroke-dashoffset",strokeMiterlimit:"stroke-miterlimit",strokeOpacity:"stroke-opacity",fillOpacity:"fill-opacity",fillRule:"fill-rule",clipPath:"clip-path",clipRule:"clip-rule",colorInterpolation:"color-interpolation",colorInterpolationFilters:"color-interpolation-filters",floodColor:"flood-color",floodOpacity:"flood-opacity",lightingColor:"lighting-color",stopColor:"stop-color",stopOpacity:"stop-opacity",shapeRendering:"shape-rendering",imageRendering:"image-rendering",textRendering:"text-rendering",pointerEvents:"pointer-events",vectorEffect:"vector-effect",paintOrder:"paint-order",fontFamily:"font-family",fontSize:"font-size",fontStyle:"font-style",fontVariant:"font-variant",fontWeight:"font-weight",fontStretch:"font-stretch",textAnchor:"text-anchor",textDecoration:"text-decoration",dominantBaseline:"dominant-baseline",alignmentBaseline:"alignment-baseline",baselineShift:"baseline-shift",letterSpacing:"letter-spacing",wordSpacing:"word-spacing",writingMode:"writing-mode",markerStart:"marker-start",markerMid:"marker-mid",markerEnd:"marker-end",xlinkHref:"xlink:href",xlinkShow:"xlink:show",xlinkActuate:"xlink:actuate",xlinkType:"xlink:type",xlinkRole:"xlink:role",xlinkTitle:"xlink:title",xlinkArcrole:"xlink:arcrole",xmlBase:"xml:base",xmlLang:"xml:lang",xmlSpace:"xml:space",xmlnsXlink:"xmlns:xlink"},Fn=Symbol.for("kerfjs.SafeHtml"),qe=class{__html;__segment;[Fn]=!0;constructor(t){typeof t=="string"?(this.__segment={kind:"static",html:t},this.__html=t):(this.__segment=t,this.__html=pe(t,!1))}toString(){return this.__html}};function be(t){return typeof t=="object"&&t!==null&&t[Fn]===!0}var Bi=new Set(["area","base","br","col","embed","hr","img","input","link","meta","source","track","wbr"]);function In(t){if(t==null||typeof t=="boolean")return{kind:"static",html:""};if(Sn(t)){const n=Mi(t);if(n!==null)return{kind:"static",html:n};const i=t.value;return{kind:"static",html:i==null||typeof i=="boolean"?"":Ue(String(i))}}if(be(t))return t.__segment??{kind:"static",html:t.__html};if(typeof t=="string")return{kind:"static",html:Ue(t)};if(typeof t=="number")return{kind:"static",html:String(t)};if(Array.isArray(t))return vi(t.map(In));const e=t;throw typeof e=="object"&&e!==null&&("nodeType"in e||"outerHTML"in e)?new Error("JSX: DOM elements cannot be passed as children (the JSX runtime renders to HTML strings). Build the tree in one JSX expression and use querySelector after toElement() to get element refs."):new Error(`JSX: unsupported child of type ${Rn(t)}. Children must be SafeHtml, string, number, boolean, null, undefined, or an array of those. Common mistakes: passing a Signal/Store object directly (use signal.value or store.state.value), passing a function (call it first), or passing a Promise (await it before render).`)}function Rn(t){if(Array.isArray(t))return"array";if(typeof t=="object"&&t!==null){const e=t.constructor?.name;return e&&e!=="Object"?`object (${e})`:"object"}return typeof t}var Pi=/^[A-Za-z_:][\w.:-]*$/;function Nn(t,e,n){if(/^on[a-z]/i.test(e))throw n?new Error(`JSX: inline event handlers like ${t}={fn} are not supported by kerf's JSX → HTML-string runtime. Use event delegation from the mount root instead:

  delegate(rootEl, 'click', '[data-action="..."]', (evt, target) => { ... });
  <button data-action="...">click</button>

See docs/5-event-delegation.md for the tier-1/tier-2/tier-3 model.`):new Error(`JSX: event-handler attribute ${JSON.stringify(t)} is not allowed — an \`on*\` attribute (whether a string emitted into HTML or a signal bound via setAttribute) installs a live inline handler, an XSS vector. kerf uses event delegation: delegate(rootEl, 'click', '[data-action="..."]', handler). See docs/5-event-delegation.md.`);if(!Pi.test(e))throw new Error(`JSX: invalid attribute name ${JSON.stringify(t)}. Attribute names must be a letter/underscore/colon followed by letters, digits, or "_.:-" (e.g. class, data-id, aria-label, xlink:href). This usually means an untrusted object was spread into JSX ({...obj}) with attacker-controlled keys — validate keys first.`)}function Ve(t,e){return Ui(t,Tn[t]??t,e)}function Ui(t,e,n){if(n==null||n===!1)return"";if(Nn(t,e,typeof n=="function"),n===!0)return` ${e}`;let i;if(be(n))i=n.__html;else if(typeof n=="number")i=String(n);else if(typeof n=="string"){if($n(e,n))return kn("JSX",e,n),"";i=Di(n)}else throw new Error(`JSX: unsupported value for attribute "${t}" — got ${Rn(n)}. Attribute values must be string, number, boolean, null, undefined, or SafeHtml. Did you mean to read .value off a Signal, or stringify the object first?`);return` ${e}="${i}"`}function I(t,e){if(typeof t=="function")return t(e);const{children:n,...i}=e;let o="",r=null;for(const[s,l]of Object.entries(i)){if(Sn(l)){const c=Tn[s]??s;Nn(s,c,!1);const u=Ei(c,l);if(u!==null){(r??=[]).push(u);continue}o+=Ve(s,l.value);continue}o+=Ve(s,l)}if(r!==null&&(o+=` ${_i()}="${r.join(",")}"`),Bi.has(t))return new qe(`<${t}${o}>`);const a=n!=null?In(n):{kind:"static",html:""};return new qe(wi(a,`<${t}${o}>`,`</${t}>`))}var je=120;function qi(t){return t.length>je?t.slice(0,je)+"…":t}var Vi="http://www.w3.org/2000/svg",ji="http://www.w3.org/1998/Math/MathML",Wi=new Set(["foreignObject","desc","title"]),Hi=new Set(["mi","mo","mn","ms","mtext"]);function Xi(t){if(t==null)return null;const{namespaceURI:e,localName:n}=t;return e===Vi?Wi.has(n)?null:"svg":e===ji?Hi.has(n)?null:"math":null}function N(t,e){const n=document.createElement("template"),i=Xi(e);if(i===null)return n.innerHTML=t,{content:n.content,count:n.content.children.length};n.innerHTML=`<${i}>${t}</${i}>`;const o=n.content.firstElementChild,r=document.createDocumentFragment();for(;o.firstChild!==null;)r.appendChild(o.firstChild);return{content:r,count:r.children.length}}function ye(t,e,n){const{content:i,count:o}=N(t,n);if(o!==1)throw wt(e,t,n);return i.firstElementChild}function On(t,e){const n=new Array(e);let i=t.firstElementChild;for(let o=0;o<e;o++)n[o]=i,i=i.nextElementSibling;return n}function wt(t,e,n){const{count:i}=N(e,n),o=i===0?"produced no top-level element":`produced ${i} top-level elements; exactly one is required`;return new Error(`each(): row render at index ${t} ${o}. Each item's render must return exactly one element — wrap multiple roots in a single parent (e.g. <li>...</li>). Got HTML: ${JSON.stringify(qi(e))}`)}function Ki(t){const e=n=>!n.startsWith("k:");for(const n of[t.caches,t.bindingCounts,t.bindingSources])for(const i of Array.from(n.keys()))e(i)&&n.delete(i)}function ct(t,e,n){const i=t.moveBefore;i!==void 0&&e.isConnected?i.call(t,e,n):t.insertBefore(e,n)}function bt(t){const e=document.activeElement;if(e===null||e===document.body||!t.contains(e))return null;const n=e;let i=null,o=null;if(n.tagName==="INPUT"||n.tagName==="TEXTAREA")try{i=n.selectionStart,o=n.selectionEnd}catch{}return{el:n,selStart:i,selEnd:o}}function yt(t){if(document.activeElement!==t.el&&t.el.isConnected&&(t.el.focus(),t.selStart!==null&&t.selEnd!==null))try{t.el.setSelectionRange(t.selStart,t.selEnd)}catch{}}var Yi="id:",Gi="data-key:",T=1,Ji=3,Q=8;function xt(t){if(t.nodeType!==T)return;const e=t;if(e.id!=="")return`${Yi}${e.id}`;if(e.dataset!==void 0&&e.dataset.key!==void 0)return`${Gi}${e.dataset.key}`}var Dn=new Set;function Zi(t,e,n=Dn){if(t==null)throw new Error('morph: liveRoot is null/undefined — pass the live element, e.g. morph(document.getElementById("app")!, template). A common cause is a typo in the id or selector that returns null at runtime even though the TypeScript types say Element.');const i=Qi(e)?e:to(t,e),o=bt(t);Un(t,i,n),o!==null&&yt(o)}function Bn(t,e,n=Dn){qn(t,e,n)}function Qi(t){return typeof t=="object"&&t!==null&&t.nodeType===T}function to(t,e){const n=t.cloneNode(!1);return n.innerHTML=String(e),n}function St(t){const{dataset:e}=t;return(e.morphSkip!==void 0?"s":"")+(e.morphSkipChildren!==void 0?"c":"")+(e.morphPreserve!==void 0?"p":"")}var eo=[K,ve,zn];function We(t){if(t.nodeType!==Q)return!1;const{data:e}=t;return eo.some(n=>e.startsWith(n))}function no(t,e){return!We(t)&&!We(e)?!0:t.data===e.data}function $t(t,e){for(;t!==null&&t.nodeType===T&&e.has(t);)t=t.nextSibling;return t}function Pn(t){return t.nodeType===Q&&t.data.startsWith(K)}function He(t,e){let n=t;for(let i=t.nextSibling;i!==null&&!Pn(i);i=i.nextSibling)i.nodeType===T&&e.has(i)&&(n=i);return n.nextSibling}function Un(t,e,n){const i=new Map;for(let a=t.firstChild;a!==null;a=a.nextSibling){if(a.nodeType===T&&n.has(a))continue;const s=xt(a);s!==void 0&&i.set(s,a)}let o=$t(t.firstChild,n),r=e.firstChild;for(;r!==null;){const a=r.nextSibling;let s=null;const l=xt(r);if(l!==void 0&&i.has(l)&&(s=i.get(l),i.delete(l),s!==o?ct(t,s,o):o=$t(o.nextSibling,n)),s===null&&o!==null&&o.nodeType===r.nodeType&&no(o,r)&&(r.nodeType!==T||o.tagName===r.tagName&&xt(o)===void 0&&l===void 0&&St(o)===St(r))&&(s=o,o=$t(Pn(s)?He(s,n):o.nextSibling,n),s.nodeType===Q&&o!==null)){const c=Fi(s);c!==null&&o===c&&(o=$t(c.nextSibling,n))}if(s===null&&r.nodeType===T&&o!==null&&l===void 0){const c=r.tagName;for(let u=o.nextSibling;u!==null;u=u.nextSibling){if(u.nodeType!==T)continue;const f=u;if(!n.has(f)&&!(f.tagName!==c||xt(f)!==void 0)&&St(f)===St(r)){s=f,ct(t,f,o);break}}}if(s===null&&o!==null&&r.nodeType===Q&&r.data.startsWith(K)){const c=r.data;for(let u=o.nextSibling;u!==null;u=u.nextSibling){if(u.nodeType!==Q||u.data!==c)continue;const f=He(u,n),w=[];for(let m=u;m!==null&&m!==f;m=m.nextSibling)w.push(m);const v=bt(t);for(const m of w)ct(t,m,o);v!==null&&yt(v),s=u;break}}if(s!==null)io(s,r,n);else{const c=r.cloneNode(!0);t.insertBefore(c,o)}r=a}for(;o!==null;){const a=o.nextSibling;if(o.nodeType===T){const s=o;!n.has(s)&&s.dataset.morphPreserve===void 0&&t.removeChild(o)}else t.removeChild(o);o=a}}function io(t,e,n){if(t.nodeType===T){qn(t,e,n);return}if(t.nodeType===Ji||t.nodeType===Q){const i=t,o=e;i.data!==o.data&&(i.data=o.data)}}function qn(t,e,n){if(t.tagName!==e.tagName){const o=e.cloneNode(!0);t.parentNode?.replaceChild(o,t);return}if(t.dataset.morphSkip!==void 0||t.isEqualNode(e))return;if(t===document.activeElement){const o=t.getAttribute("contenteditable");if(o!==null&&o.toLowerCase()!=="false")return;ao(t)&&so(t,e)}if(ro(t,e),t.dataset.morphSkipChildren!==void 0)return;const i=t.tagName==="TEXTAREA"&&t!==document.activeElement&&t.textContent!==e.textContent;Un(t,e,n),i&&(t.value=e.textContent)}function oo(t,e){return e==="open"&&(t==="DETAILS"||t==="DIALOG")}function ro(t,e){const n=e.attributes;for(let r=0;r<n.length;r++){const a=n[r],s=a.namespaceURI,l=a.localName,c=a.value;s!==null?t.getAttributeNS(s,l)!==c&&t.setAttributeNS(s,a.name,c):t.getAttribute(l)!==c&&(t.setAttribute(l,c),P(t,l,c,!0))}const i=t.attributes,o=t.tagName;for(let r=i.length-1;r>=0;r--){const a=i[r],s=a.namespaceURI,l=a.localName;s!==null?e.hasAttributeNS(s,l)||t.removeAttributeNS(s,l):!e.hasAttribute(l)&&!oo(o,l)&&(t.removeAttribute(l),P(t,l,"",!1))}}function ao(t){if(t.tagName==="TEXTAREA")return!0;if(t.tagName==="INPUT"){const e=t.type;return e==="text"||e==="search"||e==="url"||e==="email"||e==="tel"||e==="password"||e===""}return!1}function so(t,e){if(t.tagName==="TEXTAREA"||t.tagName==="INPUT"){const n=t,i=e;i.value=n.value;try{i.setSelectionRange(n.selectionStart,n.selectionEnd)}catch{}}}function Et(t){return t.items.length>0?t.items[t.items.length-1].node.nextSibling:t.marker.nextSibling}var ht=60,Ce=62,Le=34,xe=39,Vn=38,Rt=61,lo=47,co=3,uo=1;function ot(t){return t===32||t===9||t===10||t===13}function Se(t,e,n){const i=e.indexOf(">"),o=n.indexOf(">");if(i===-1||o===-1||e.length-i!==n.length-o||e.slice(i)!==n.slice(o)||Nt(e)||Nt(n))return!1;const r=Ke(e,i),a=Ke(n,o);if(r===null||a===null||r.tagName!==a.tagName)return!1;for(const l of r.attrs.keys())if(l.indexOf(":")!==-1)return!1;for(const l of a.attrs.keys())if(l.indexOf(":")!==-1)return!1;const s=t.tagName;for(const[l,c]of a.attrs){if(r.attrs.get(l)===c)continue;const f=po(c);t.setAttribute(l,f),P(t,l,f,!0)}for(const l of r.attrs.keys())a.attrs.has(l)||go(s,l)||(t.removeAttribute(l),P(t,l,"",!1));return!0}function $e(t,e,n){if(Nt(e)||Nt(n))return!1;let i=0;const o=Math.min(e.length,n.length);for(;i<o&&e.charCodeAt(i)===n.charCodeAt(i);)i++;let r=0;const a=o-i;for(;r<a&&e.charCodeAt(e.length-1-r)===n.charCodeAt(n.length-1-r);)r++;const s=e.length-r,l=n.length-r;if(!Xe(e,i,s)||!Xe(n,i,l)||i===0)return!1;const c=e.charCodeAt(i-1);if(c===ht||c===Le||c===xe||c===Rt||c===Vn)return!1;const u=ho(e,Ce,i-1);if(u===-1)return!1;const f=e.indexOf("<",i);if(f===-1||f<s)return!1;const w=f+(n.length-e.length),v=e.slice(u+1,f),m=n.slice(u+1,w);if(e.lastIndexOf("<!--kfb",u)!==-1)return!1;const A=fo(e,u+1),$=mo(t,A);if($===null||$.nodeValue!==v)return!1;$.nodeValue=m;const M=$.parentNode;return M!==null&&M.tagName==="TEXTAREA"&&M!==document.activeElement&&(M.value=m),!0}function Nt(t){return t.indexOf("data-morph-skip")!==-1}function Xe(t,e,n){for(let i=e;i<n;i++){const o=t.charCodeAt(i);if(o===ht||o===Ce||o===Le||o===xe||o===Vn||o===Rt)return!1}return!0}function ho(t,e,n){for(let i=n;i>=0;i--)if(t.charCodeAt(i)===e)return i;return-1}function fo(t,e){let n=0,i=0;for(;i<e;)if(t.charCodeAt(i)===ht){for(;i<e&&t.charCodeAt(i)!==Ce;)i++;i++}else{const o=i;for(;i<e&&t.charCodeAt(i)!==ht;)i++;i>o&&n++}return n}function mo(t,e){let n=0,i=null;function o(r){for(let a=r.firstChild;a!==null;a=a.nextSibling){if(i!==null)return;if(a.nodeType===co){if(n===e){i=a;return}n++}else a.nodeType===uo&&o(a)}}return o(t),i}function Ke(t,e){if(t.charCodeAt(0)!==ht)return null;let n=1,i=e;n<i&&t.charCodeAt(i-1)===lo&&(i-=1);const o=n;for(;n<i;){const s=t.charCodeAt(n);if(ot(s))break;n++}const r=t.slice(o,n);if(r.length===0)return null;const a=new Map;for(;n<i;){for(;n<i&&ot(t.charCodeAt(n));)n++;if(n>=i)break;const s=n;for(;n<i;){const c=t.charCodeAt(n);if(c===Rt||ot(c))break;n++}const l=t.slice(s,n);if(l.length===0)return null;for(;n<i&&ot(t.charCodeAt(n));)n++;if(n<i&&t.charCodeAt(n)===Rt){for(n++;n<i&&ot(t.charCodeAt(n));)n++;if(n>=i)return null;const c=t.charCodeAt(n);if(c!==Le&&c!==xe)return null;n++;const u=n;for(;n<i&&t.charCodeAt(n)!==c;)n++;if(n>=i)return null;a.set(l,t.slice(u,n)),n++}else a.set(l,"")}return{tagName:r,attrs:a}}function po(t){return t.indexOf("&")===-1?t:t.replace(/&quot;/g,'"').replace(/&#39;/g,"'").replace(/&lt;/g,"<").replace(/&gt;/g,">").replace(/&amp;/g,"&")}function go(t,e){return e==="open"&&(t==="DETAILS"||t==="DIALOG")}function vo(t,e){const{liveParent:n}=t,i=t.items,o=bt(n);let r=0;for(;r<e.length;){const a=e[r];if(a.type==="replace"){r+=1;continue}if(a.type==="update"){let s=r+1;for(;s<e.length&&e[s].type==="update";)s+=1;s-r===1?bo(n,i,a):yo(n,i,e,r,s),r=s;continue}if(a.type==="insert"){let s=r+1;for(;s<e.length&&e[s].type==="insert"&&e[s].index===e[s-1].index+1;)s+=1;s-r===1?wo(n,i,a,Et(t)):Co(n,i,e,r,s,Et(t)),r=s;continue}if(a.type==="remove"){const s=i[a.index];U(s.bindingDisposers),n.removeChild(s.node),i.splice(a.index,1),r+=1;continue}if(a.type==="move"){const s=i[a.from];let l=a.to;a.from<a.to&&(l+=1);const c=l<i.length?i[l].node:Et(t);ct(n,s.node,c),i.splice(a.from,1),i.splice(a.to,0,s),r+=1;continue}}o!==null&&yt(o),i.length>0&&L.missingRowKey?.(i[0].node,i[0].html,t)}function wo(t,e,n,i){const{html:o}=n,r=ye(o,n.index,t),a=n.index<e.length?e[n.index].node:i;t.insertBefore(r,a),e.splice(n.index,0,{ref:n.item,cacheKey:void 0,html:o,node:r,bindings:n.bindings,bindingDisposers:ke(r,n.bindings)})}function ke(t,e){return e!==void 0&&e.length>0?Ut(t,e):void 0}function bo(t,e,n){const{html:i}=n,o=e[n.index];if(i===o.html){e[n.index]=ft(n,i,o);return}if(Se(o.node,o.html,i)||$e(o.node,o.html,i)){e[n.index]=ft(n,i,o);return}const r=ye(i,n.index,t);jn(t,e,n,i,r)}function jn(t,e,n,i,o){const r=e[n.index];r.node.tagName===o.tagName?(Bn(r.node,o),e[n.index]=ft(n,i,r)):(U(r.bindingDisposers),t.replaceChild(o,r.node),e[n.index]={ref:n.item,cacheKey:void 0,html:i,node:o,bindings:n.bindings,bindingDisposers:ke(o,n.bindings)})}function ft(t,e,n){const i=zt(n.node,n.bindings,n.bindingDisposers,t.bindings);return{ref:t.item,cacheKey:void 0,html:e,node:n.node,bindings:i.bindings,bindingDisposers:i.bindingDisposers}}function yo(t,e,n,i,o){const r=[];for(let c=i;c<o;c++){const u=n[c],f=e[u.index];if(u.html===f.html){e[u.index]=ft(u,u.html,f);continue}if(Se(f.node,f.html,u.html)||$e(f.node,f.html,u.html)){e[u.index]=ft(u,u.html,f);continue}r.push({patchIdx:c,html:u.html})}if(r.length===0)return;const{content:a,count:s}=N(r.map(c=>c.html).join(""),t);if(s!==r.length)throw xo(n,r,t);const l=On(a,r.length);for(let c=0;c<r.length;c++){const u=r[c],f=n[u.patchIdx];jn(t,e,f,u.html,l[c])}}function Co(t,e,n,i,o,r){const a=n[i].index,s=new Array(o-i);for(let v=i;v<o;v++)s[v-i]=n[v].html;const{content:l,count:c}=N(s.join(""),t);if(c!==s.length)throw Lo(n,i,s,t);const u=On(l,o-i),f=a<e.length?e[a].node:r;t.insertBefore(l,f);const w=new Array(o-i);for(let v=0;v<w.length;v++){const m=n[i+v];w[v]={ref:m.item,cacheKey:void 0,html:s[v],node:u[v],bindings:m.bindings,bindingDisposers:ke(u[v],m.bindings)}}e.splice(a,0,...w)}function Lo(t,e,n,i){for(let o=0;o<n.length;o++)if(N(n[o],i).count!==1)return wt(t[e+o].index,n[o],i);return new Error("each(): bulk-insert mismatch with no per-row offender (kerf bug).")}function xo(t,e,n){for(const i of e)if(N(i.html,n).count!==1)return wt(t[i.patchIdx].index,i.html,n);return new Error("each(): bulk-update mismatch with no per-row offender (kerf bug).")}function So(t,e){const n=t.items,i=e.items,o=i.length;if(o===0||o!==n.length)return!1;for(let l=0;l<o;l++)if(i[l].ref!==n[l].ref)return!1;const{liveParent:r}=t,a=new Array(o),s=bt(r);for(let l=0;l<o;l++)a[l]=$o(r,n[l],i[l],l);return s!==null&&yt(s),t.items=a,L.missingRowKey?.(a[0].node,a[0].html,t),!0}function $o(t,e,n,i){if(e.html===n.html||Se(e.node,e.html,n.html)||$e(e.node,e.html,n.html)){const a=zt(e.node,e.bindings,e.bindingDisposers,n.bindings);return{ref:n.ref,cacheKey:n.cacheKey,html:n.html,node:e.node,bindings:a.bindings,bindingDisposers:a.bindingDisposers}}const o=ye(n.html,i,t);if(e.node.tagName===o.tagName){Bn(e.node,o);const a=zt(e.node,e.bindings,e.bindingDisposers,n.bindings);return{ref:n.ref,cacheKey:n.cacheKey,html:n.html,node:e.node,bindings:a.bindings,bindingDisposers:a.bindingDisposers}}U(e.bindingDisposers),t.replaceChild(o,e.node);const r=zt(o,void 0,void 0,n.bindings);return{ref:n.ref,cacheKey:n.cacheKey,html:n.html,node:o,bindings:r.bindings,bindingDisposers:r.bindingDisposers}}function ko(t,e){if(So(t,e))return;const{liveParent:n}=t,{newRecord:i,prevIdx:o,removedItems:r,freshIndices:a,freshHtmls:s}=Ao(t.items,e),l=Et(t);zo(i,a,s,n);const c=bt(n);_o(n,r),Mo(n,i,o,To(o),l),c!==null&&yt(c),t.items=i,i.length>0&&L.missingRowKey?.(i[0].node,i[0].html,t)}function Ao(t,e){const n=new Map;for(let l=0;l<t.length;l++)n.set(t[l].ref,[t[l],l]);const i=new Array(e.items.length),o=new Array(e.items.length),r=[],a=[],s=[];for(let l=0;l<e.items.length;l++){const c=e.items[l],u=n.get(c.ref);if(u!==void 0){if(n.delete(c.ref),u[0].html===c.html){i[l]=u[0],o[l]=u[1];continue}r.push(u[0])}i[l]={ref:c.ref,cacheKey:c.cacheKey,html:c.html,node:null,bindings:c.bindings},o[l]=-1,a.push(l),s.push(c.html)}for(const[,l]of n)r.push(l[0]);return{newRecord:i,prevIdx:o,removedItems:r,freshIndices:a,freshHtmls:s}}function zo(t,e,n,i){if(n.length===0)return;const{content:o,count:r}=N(n.join(""),i);if(r!==n.length)throw Eo(t,e,n,i);let a=o.firstElementChild;for(const s of e){const l=a.nextElementSibling,c=t[s];c.node=a,c.bindings!==void 0&&c.bindings.length>0&&(c.bindingDisposers=Ut(c.node,c.bindings)),a=l}}function Eo(t,e,n,i){for(let o=0;o<n.length;o++)if(N(n[o],i).count!==1)return wt(e[o],t[e[o]].html,i);return new Error("each(): bulk-parse mismatch with no per-row offender (kerf bug).")}function _o(t,e){for(const n of e)U(n.bindingDisposers),n.node.parentElement===t&&t.removeChild(n.node)}function Mo(t,e,n,i,o){let r=o;for(let a=e.length-1;a>=0;a--){const s=e[a].node;(n[a]===-1||!i.has(a))&&ct(t,s,r),r=s}}function To(t){const e=[],n=[],i=new Array(t.length);for(let a=0;a<t.length;a++){const s=t[a];if(s===-1){i[a]=-1;continue}let l=0,c=e.length;for(;l<c;){const u=l+c>>1;e[u]<s?l=u+1:c=u}i[a]=l>0?n[l-1]:-1,e[l]=s,n[l]=a}const o=new Set;let r=n.length>0?n[n.length-1]:-1;for(;r!==-1;)o.add(r),r=i[r];return o}function Fo(t,e){if(e.patches!==void 0&&t.items.length>0){vo(t,e.patches);return}ko(t,e)}var ie=Symbol.for("kerfjs.mounted"),Ye="mount: rootEl is already inside (or contains) a mounted tree. kerf supports one mount per tree — compose with plain functions that return JSX instead of nesting mounts.";function Xt(t){return t[ie]===!0}function Ge(t,e){e?t[ie]=!0:delete t[ie]}function Io(t){const e=t.tagName.toLowerCase(),n=t.id?`#${t.id}`:"";return`<${e}${n}>`}function Ro(t){if(Xt(t))throw new Error(`mount: ${Io(t)} is already mounted. Call the disposer returned by the first mount() before mounting again. kerf supports one mount per element — compose with plain functions that return JSX instead of nesting mounts.`);let e=t.parentElement;for(;e!==null;){if(Xt(e))throw new Error(Ye);e=e.parentElement}const n=[];for(let i=0;i<t.children.length;i++)n.push(t.children[i]);for(;n.length>0;){const i=n.pop();if(Xt(i))throw new Error(Ye);for(let o=0;o<i.children.length;o++)n.push(i.children[o])}}function No(t,e){if(t==null)throw new Error('mount: rootEl is null/undefined — pass the live element, e.g. mount(document.getElementById("app")!, render). A common cause is a typo in the id or selector that returns null at runtime even though the TypeScript types say HTMLElement.');const n=t.ownerDocument;n!==document&&n.defaultView===null&&document.adoptNode(t),Ro(t),Ge(t,!0);const i=L.listenerRebuild?.(t)??null,o=new Map,r={counter:0,caches:new Map,bindingCounts:new Map,bindingSources:new Map,keysThisRender:new Set,shiftCandidates:[],warnedShiftIds:new Set,rebuiltLists:new Set},a=zi();let s=[],l=[],c=!0,u="";const f={warned:!1},w=()=>{r.counter=0,r.keysThisRender.clear(),r.shiftCandidates.length=0,a.counter=0,a.list=[],Be(a);try{return e()}finally{Be(null)}},v=me(()=>{let m=w();if(r.previousCallCount!==void 0&&r.previousCallCount!==r.counter){for(const x of r.shiftCandidates)r.warnedShiftIds.has(x)||(r.warnedShiftIds.add(x),L.listIdShift?.(x));Ki(r),m=w()}let $=Ze(m);if(c)Oo(t,$,o),u=ge($),L.parserRepair?.(u),s=Pe(t,a,s),L.staleBindingEnabled?.()===!0&&(l=a.list),c=!1;else{let x=Je(t,$,o,r,u,f);if(Bo($,r.rebuiltLists)){for(const it of r.rebuiltLists)r.bindingCounts.delete(it);m=w(),$=Ze(m),x=Je(t,$,o,r,u,f)}x!==u?(s=Pe(t,a,s),L.staleBindingEnabled?.()===!0&&(l=a.list)):L.staleBinding?.(l,a.list),u=x}const M=L.listInvariantsEnabled?.()===!0?new Map:null;for(const x of vt($).values()){const it=o.get(x.id);if(it===void 0)throw new Error("mount: an each() list appeared in the render output but its marker never reached the live DOM. The most common cause is an each() introduced inside a data-morph-skip subtree on a re-render — the morph leaves that subtree untouched, so the list can never bind. Move the each() outside the skipped subtree, or remove data-morph-skip from its ancestor.");Fo(it,x),r.bindingCounts.set(x.id,it.items.length),r.bindingSources.set(x.id,x.source),M?.set(x.id,x.patches!==void 0&&x.source!==void 0?x.source.value.length:x.items.length)}r.previousCallCount=r.counter,L.listInvariants?.(t,o,M??void 0)});return()=>{v();for(const m of s)m();s=[];for(const m of o.values())for(const A of m.items)U(A.bindingDisposers);i?.disconnect(),Ge(t,!1)}}function Oo(t,e,n){t.innerHTML=pe(e,!0),Wn(t,e,n,!0)}function Je(t,e,n,i,o,r){i.rebuiltLists.clear();const a=ge(e);if(a===o)return o;L.valueOnlyRerender?.(o,a,r),Vo(e,n,i);const s=t.cloneNode(!1);return s.innerHTML=a,Zi(t,s,qo(n)),Wn(t,e,n,!1,i.rebuiltLists),a}function Do(t){return t==null||t===!1||t===!0?"":String(t)}function Ze(t){return be(t)?t.__segment??{kind:"static",html:t.__html}:{kind:"static",html:Do(t)}}function Bo(t,e){if(e.size===0)return!1;const n=vt(t);for(const i of e)if(n.get(i)?.patches!==void 0)return!0;return!1}function Wn(t,e,n,i,o){const r=vt(e),a=[];Hn(t,a);for(const s of a){if(!s.data.startsWith(K))continue;const l=s.data.slice(K.length),c=n.get(l);if(c!==void 0){if(c.marker===s&&t.contains(c.marker))continue;for(const m of c.items)U(m.bindingDisposers),t.contains(m.node)&&m.node.parentElement?.removeChild(m.node);n.delete(l),o?.add(l),L.listRebind?.(l,s.parentElement)}const u=r.get(l),f=s.parentElement,w=[];if(i){let m=s.nextElementSibling;for(let A=0;A<u.items.length&&m!==null;A++){Po(u.items[A].html,A,m,f);const $=u.items[A].bindings,M={ref:u.items[A].ref,cacheKey:u.items[A].cacheKey,html:u.items[A].html,node:m,bindings:$};$!==void 0&&$.length>0&&(M.bindingDisposers=Ut(m,$)),w.push(M),m=m.nextElementSibling}}const v={liveParent:f,items:w,marker:s};w.length>0&&L.missingRowKey?.(w[0].node,w[0].html,v),L.eachInMorphSkip?.(l,f,t),n.set(l,v)}}function Po(t,e,n,i){if(n.outerHTML===t)return;const{content:o,count:r}=N(t,i);if(r!==1)throw wt(e,t,i);const a=o.firstElementChild.tagName;if(n.tagName!==a)throw Uo(e,n.tagName,a)}function Uo(t,e,n){const i=e.toLowerCase(),o=n.toLowerCase();return new Error(`each(): row ${t} renders <${o}>, but the HTML parser wrapped the rows in <${i}> — so kerf cannot bind one row per element. This happens when an each() of <${o}> sits directly inside a table: the parser inserts <${i}> around the whole run. Put the each() inside an explicit <${i}> (e.g. <table><${i}>{each(...)}</${i}></table>) so the rows are the direct children kerf binds.`)}function qo(t){const e=new Set;for(const n of t.values())for(const i of n.items)e.add(i.node);return e}function Vo(t,e,n){const i=vt(t);for(const[o,r]of e)if(!i.has(o)){for(const a of r.items)U(a.bindingDisposers),a.node.parentElement!==null&&a.node.parentElement.removeChild(a.node);r.marker.parentElement!==null&&r.marker.parentElement.removeChild(r.marker),e.delete(o),n.bindingCounts.delete(o),n.bindingSources.delete(o),n.caches.delete(o)}}function Hn(t,e){for(let n=t.firstChild;n!==null;n=n.nextSibling)n.nodeType===Node.COMMENT_NODE?e.push(n):n.nodeType===Node.ELEMENT_NODE&&Hn(n,e)}var jo=new Set(["focus","blur","scroll","load","error","mouseenter","mouseleave"]);function Wo(t,e){try{document.createElement("div").matches(t)}catch{throw new Error(`${e}: invalid selector "${t}". Pass a valid CSS selector (e.g. '[data-action="add"]', '.btn', 'input').`)}}function Ho(t,e,n,i){return o=>{const r=o.target;if(!(r instanceof Element))return;const a=r.closest(e);a!==null&&t.contains(a)&&n(o,a)}}function Xn(t,e,n,i,o){Wo(n,"delegate"),L.delegateInEffect?.("delegate");const r=Ho(t,n,i),a=jo.has(e);return t.addEventListener(e,r,a),()=>{t.removeEventListener(e,r,a)}}var Kn=()=>({checkValidity(t){const e=t.input,n={message:"",isValid:!0,invalidKeys:[]};if(!e)return n;let i=!0;if("checkValidity"in e&&(i=e.checkValidity()),i)return n;if(n.isValid=!1,"validationMessage"in e&&(n.message=e.validationMessage),!("validity"in e))return n.invalidKeys.push("customError"),n;for(const o in e.validity){if(o==="valid")continue;const r=o;e.validity[r]&&n.invalidKeys.push(r)}return n}});var Yn=class extends Event{constructor(){super("wa-invalid",{bubbles:!0,cancelable:!1,composed:!0})}};var Xo=Object.defineProperty,Ko=Object.getOwnPropertyDescriptor,Gn=t=>{throw TypeError(t)},d=(t,e,n,i)=>{for(var o=i>1?void 0:i?Ko(e,n):e,r=t.length-1,a;r>=0;r--)(a=t[r])&&(o=(i?a(e,n,o):a(o))||o);return i&&o&&Xo(e,n,o),o},Jn=(t,e,n)=>e.has(t)||Gn("Cannot "+n),Yo=(t,e,n)=>(Jn(t,e,"read from private field"),e.get(t)),Go=(t,e,n)=>e.has(t)?Gn("Cannot add the same private member more than once"):e instanceof WeakSet?e.add(t):e.set(t,n),Jo=(t,e,n,i)=>(Jn(t,e,"write to private field"),e.set(t,n),n);const _t=globalThis,Ae=_t.ShadowRoot&&(_t.ShadyCSS===void 0||_t.ShadyCSS.nativeShadow)&&"adoptedStyleSheets"in Document.prototype&&"replace"in CSSStyleSheet.prototype,ze=Symbol(),Qe=new WeakMap;let Zn=class{constructor(e,n,i){if(this._$cssResult$=!0,i!==ze)throw Error("CSSResult is not constructable. Use `unsafeCSS` or `css` instead.");this.cssText=e,this.t=n}get styleSheet(){let e=this.o;const n=this.t;if(Ae&&e===void 0){const i=n!==void 0&&n.length===1;i&&(e=Qe.get(n)),e===void 0&&((this.o=e=new CSSStyleSheet).replaceSync(this.cssText),i&&Qe.set(n,e))}return e}toString(){return this.cssText}};const Zo=t=>new Zn(typeof t=="string"?t:t+"",void 0,ze),O=(t,...e)=>{const n=t.length===1?t[0]:e.reduce((i,o,r)=>i+(a=>{if(a._$cssResult$===!0)return a.cssText;if(typeof a=="number")return a;throw Error("Value passed to 'css' function must be a 'css' function result: "+a+". Use 'unsafeCSS' to pass non-literal values, but take care to ensure page security.")})(o)+t[r+1],t[0]);return new Zn(n,t,ze)},Qo=(t,e)=>{if(Ae)t.adoptedStyleSheets=e.map(n=>n instanceof CSSStyleSheet?n:n.styleSheet);else for(const n of e){const i=document.createElement("style"),o=_t.litNonce;o!==void 0&&i.setAttribute("nonce",o),i.textContent=n.cssText,t.appendChild(i)}},tn=Ae?t=>t:t=>t instanceof CSSStyleSheet?(e=>{let n="";for(const i of e.cssRules)n+=i.cssText;return Zo(n)})(t):t;const{is:tr,defineProperty:er,getOwnPropertyDescriptor:nr,getOwnPropertyNames:ir,getOwnPropertySymbols:or,getPrototypeOf:rr}=Object,qt=globalThis,en=qt.trustedTypes,ar=en?en.emptyScript:"",sr=qt.reactiveElementPolyfillSupport,dt=(t,e)=>t,Ot={toAttribute(t,e){switch(e){case Boolean:t=t?ar:null;break;case Object:case Array:t=t==null?t:JSON.stringify(t)}return t},fromAttribute(t,e){let n=t;switch(e){case Boolean:n=t!==null;break;case Number:n=t===null?null:Number(t);break;case Object:case Array:try{n=JSON.parse(t)}catch{n=null}}return n}},Ee=(t,e)=>!tr(t,e),nn={attribute:!0,type:String,converter:Ot,reflect:!1,useDefault:!1,hasChanged:Ee};Symbol.metadata??=Symbol("metadata"),qt.litPropertyMetadata??=new WeakMap;let J=class extends HTMLElement{static addInitializer(e){this._$Ei(),(this.l??=[]).push(e)}static get observedAttributes(){return this.finalize(),this._$Eh&&[...this._$Eh.keys()]}static createProperty(e,n=nn){if(n.state&&(n.attribute=!1),this._$Ei(),this.prototype.hasOwnProperty(e)&&((n=Object.create(n)).wrapped=!0),this.elementProperties.set(e,n),!n.noAccessor){const i=Symbol(),o=this.getPropertyDescriptor(e,i,n);o!==void 0&&er(this.prototype,e,o)}}static getPropertyDescriptor(e,n,i){const{get:o,set:r}=nr(this.prototype,e)??{get(){return this[n]},set(a){this[n]=a}};return{get:o,set(a){const s=o?.call(this);r?.call(this,a),this.requestUpdate(e,s,i)},configurable:!0,enumerable:!0}}static getPropertyOptions(e){return this.elementProperties.get(e)??nn}static _$Ei(){if(this.hasOwnProperty(dt("elementProperties")))return;const e=rr(this);e.finalize(),e.l!==void 0&&(this.l=[...e.l]),this.elementProperties=new Map(e.elementProperties)}static finalize(){if(this.hasOwnProperty(dt("finalized")))return;if(this.finalized=!0,this._$Ei(),this.hasOwnProperty(dt("properties"))){const n=this.properties,i=[...ir(n),...or(n)];for(const o of i)this.createProperty(o,n[o])}const e=this[Symbol.metadata];if(e!==null){const n=litPropertyMetadata.get(e);if(n!==void 0)for(const[i,o]of n)this.elementProperties.set(i,o)}this._$Eh=new Map;for(const[n,i]of this.elementProperties){const o=this._$Eu(n,i);o!==void 0&&this._$Eh.set(o,n)}this.elementStyles=this.finalizeStyles(this.styles)}static finalizeStyles(e){const n=[];if(Array.isArray(e)){const i=new Set(e.flat(1/0).reverse());for(const o of i)n.unshift(tn(o))}else e!==void 0&&n.push(tn(e));return n}static _$Eu(e,n){const i=n.attribute;return i===!1?void 0:typeof i=="string"?i:typeof e=="string"?e.toLowerCase():void 0}constructor(){super(),this._$Ep=void 0,this.isUpdatePending=!1,this.hasUpdated=!1,this._$Em=null,this._$Ev()}_$Ev(){this._$ES=new Promise(e=>this.enableUpdating=e),this._$AL=new Map,this._$E_(),this.requestUpdate(),this.constructor.l?.forEach(e=>e(this))}addController(e){(this._$EO??=new Set).add(e),this.renderRoot!==void 0&&this.isConnected&&e.hostConnected?.()}removeController(e){this._$EO?.delete(e)}_$E_(){const e=new Map,n=this.constructor.elementProperties;for(const i of n.keys())this.hasOwnProperty(i)&&(e.set(i,this[i]),delete this[i]);e.size>0&&(this._$Ep=e)}createRenderRoot(){const e=this.shadowRoot??this.attachShadow(this.constructor.shadowRootOptions);return Qo(e,this.constructor.elementStyles),e}connectedCallback(){this.renderRoot??=this.createRenderRoot(),this.enableUpdating(!0),this._$EO?.forEach(e=>e.hostConnected?.())}enableUpdating(e){}disconnectedCallback(){this._$EO?.forEach(e=>e.hostDisconnected?.())}attributeChangedCallback(e,n,i){this._$AK(e,i)}_$ET(e,n){const i=this.constructor.elementProperties.get(e),o=this.constructor._$Eu(e,i);if(o!==void 0&&i.reflect===!0){const r=(i.converter?.toAttribute!==void 0?i.converter:Ot).toAttribute(n,i.type);this._$Em=e,r==null?this.removeAttribute(o):this.setAttribute(o,r),this._$Em=null}}_$AK(e,n){const i=this.constructor,o=i._$Eh.get(e);if(o!==void 0&&this._$Em!==o){const r=i.getPropertyOptions(o),a=typeof r.converter=="function"?{fromAttribute:r.converter}:r.converter?.fromAttribute!==void 0?r.converter:Ot;this._$Em=o;const s=a.fromAttribute(n,r.type);this[o]=s??this._$Ej?.get(o)??s,this._$Em=null}}requestUpdate(e,n,i,o=!1,r){if(e!==void 0){const a=this.constructor;if(o===!1&&(r=this[e]),i??=a.getPropertyOptions(e),!((i.hasChanged??Ee)(r,n)||i.useDefault&&i.reflect&&r===this._$Ej?.get(e)&&!this.hasAttribute(a._$Eu(e,i))))return;this.C(e,n,i)}this.isUpdatePending===!1&&(this._$ES=this._$EP())}C(e,n,{useDefault:i,reflect:o,wrapped:r},a){i&&!(this._$Ej??=new Map).has(e)&&(this._$Ej.set(e,a??n??this[e]),r!==!0||a!==void 0)||(this._$AL.has(e)||(this.hasUpdated||i||(n=void 0),this._$AL.set(e,n)),o===!0&&this._$Em!==e&&(this._$Eq??=new Set).add(e))}async _$EP(){this.isUpdatePending=!0;try{await this._$ES}catch(n){Promise.reject(n)}const e=this.scheduleUpdate();return e!=null&&await e,!this.isUpdatePending}scheduleUpdate(){return this.performUpdate()}performUpdate(){if(!this.isUpdatePending)return;if(!this.hasUpdated){if(this.renderRoot??=this.createRenderRoot(),this._$Ep){for(const[o,r]of this._$Ep)this[o]=r;this._$Ep=void 0}const i=this.constructor.elementProperties;if(i.size>0)for(const[o,r]of i){const{wrapped:a}=r,s=this[o];a!==!0||this._$AL.has(o)||s===void 0||this.C(o,void 0,r,s)}}let e=!1;const n=this._$AL;try{e=this.shouldUpdate(n),e?(this.willUpdate(n),this._$EO?.forEach(i=>i.hostUpdate?.()),this.update(n)):this._$EM()}catch(i){throw e=!1,this._$EM(),i}e&&this._$AE(n)}willUpdate(e){}_$AE(e){this._$EO?.forEach(n=>n.hostUpdated?.()),this.hasUpdated||(this.hasUpdated=!0,this.firstUpdated(e)),this.updated(e)}_$EM(){this._$AL=new Map,this.isUpdatePending=!1}get updateComplete(){return this.getUpdateComplete()}getUpdateComplete(){return this._$ES}shouldUpdate(e){return!0}update(e){this._$Eq&&=this._$Eq.forEach(n=>this._$ET(n,this[n])),this._$EM()}updated(e){}firstUpdated(e){}};J.elementStyles=[],J.shadowRootOptions={mode:"open"},J[dt("elementProperties")]=new Map,J[dt("finalized")]=new Map,sr?.({ReactiveElement:J}),(qt.reactiveElementVersions??=[]).push("2.1.2");const _e=globalThis,on=t=>t,Dt=_e.trustedTypes,rn=Dt?Dt.createPolicy("lit-html",{createHTML:t=>t}):void 0,Qn="$lit$",B=`lit$${Math.random().toFixed(9).slice(2)}$`,ti="?"+B,lr=`<${ti}>`,Y=document,mt=()=>Y.createComment(""),pt=t=>t===null||typeof t!="object"&&typeof t!="function",Me=Array.isArray,cr=t=>Me(t)||typeof t?.[Symbol.iterator]=="function",Kt=`[ 	
\f\r]`,rt=/<(?:(!--|\/[^a-zA-Z])|(\/?[a-zA-Z][^>\s]*)|(\/?$))/g,an=/-->/g,sn=/>/g,q=RegExp(`>|${Kt}(?:([^\\s"'>=/]+)(${Kt}*=${Kt}*(?:[^ 	
\f\r"'\`<>=]|("|')|))|$)`,"g"),ln=/'/g,cn=/"/g,ei=/^(?:script|style|textarea|title)$/i,dr=t=>(e,...n)=>({_$litType$:t,strings:e,values:n}),z=dr(1),E=Symbol.for("lit-noChange"),C=Symbol.for("lit-nothing"),dn=new WeakMap,j=Y.createTreeWalker(Y,129);function ni(t,e){if(!Me(t)||!t.hasOwnProperty("raw"))throw Error("invalid template strings array");return rn!==void 0?rn.createHTML(e):e}const ur=(t,e)=>{const n=t.length-1,i=[];let o,r=e===2?"<svg>":e===3?"<math>":"",a=rt;for(let s=0;s<n;s++){const l=t[s];let c,u,f=-1,w=0;for(;w<l.length&&(a.lastIndex=w,u=a.exec(l),u!==null);)w=a.lastIndex,a===rt?u[1]==="!--"?a=an:u[1]!==void 0?a=sn:u[2]!==void 0?(ei.test(u[2])&&(o=RegExp("</"+u[2],"g")),a=q):u[3]!==void 0&&(a=q):a===q?u[0]===">"?(a=o??rt,f=-1):u[1]===void 0?f=-2:(f=a.lastIndex-u[2].length,c=u[1],a=u[3]===void 0?q:u[3]==='"'?cn:ln):a===cn||a===ln?a=q:a===an||a===sn?a=rt:(a=q,o=void 0);const v=a===q&&t[s+1].startsWith("/>")?" ":"";r+=a===rt?l+lr:f>=0?(i.push(c),l.slice(0,f)+Qn+l.slice(f)+B+v):l+B+(f===-2?s:v)}return[ni(t,r+(t[n]||"<?>")+(e===2?"</svg>":e===3?"</math>":"")),i]};class gt{constructor({strings:e,_$litType$:n},i){let o;this.parts=[];let r=0,a=0;const s=e.length-1,l=this.parts,[c,u]=ur(e,n);if(this.el=gt.createElement(c,i),j.currentNode=this.el.content,n===2||n===3){const f=this.el.content.firstChild;f.replaceWith(...f.childNodes)}for(;(o=j.nextNode())!==null&&l.length<s;){if(o.nodeType===1){if(o.hasAttributes())for(const f of o.getAttributeNames())if(f.endsWith(Qn)){const w=u[a++],v=o.getAttribute(f).split(B),m=/([.?@])?(.*)/.exec(w);l.push({type:1,index:r,name:m[2],strings:v,ctor:m[1]==="."?fr:m[1]==="?"?mr:m[1]==="@"?pr:Vt}),o.removeAttribute(f)}else f.startsWith(B)&&(l.push({type:6,index:r}),o.removeAttribute(f));if(ei.test(o.tagName)){const f=o.textContent.split(B),w=f.length-1;if(w>0){o.textContent=Dt?Dt.emptyScript:"";for(let v=0;v<w;v++)o.append(f[v],mt()),j.nextNode(),l.push({type:2,index:++r});o.append(f[w],mt())}}}else if(o.nodeType===8)if(o.data===ti)l.push({type:2,index:r});else{let f=-1;for(;(f=o.data.indexOf(B,f+1))!==-1;)l.push({type:7,index:r}),f+=B.length-1}r++}}static createElement(e,n){const i=Y.createElement("template");return i.innerHTML=e,i}}function tt(t,e,n=t,i){if(e===E)return e;let o=i!==void 0?n._$Co?.[i]:n._$Cl;const r=pt(e)?void 0:e._$litDirective$;return o?.constructor!==r&&(o?._$AO?.(!1),r===void 0?o=void 0:(o=new r(t),o._$AT(t,n,i)),i!==void 0?(n._$Co??=[])[i]=o:n._$Cl=o),o!==void 0&&(e=tt(t,o._$AS(t,e.values),o,i)),e}class hr{constructor(e,n){this._$AV=[],this._$AN=void 0,this._$AD=e,this._$AM=n}get parentNode(){return this._$AM.parentNode}get _$AU(){return this._$AM._$AU}u(e){const{el:{content:n},parts:i}=this._$AD,o=(e?.creationScope??Y).importNode(n,!0);j.currentNode=o;let r=j.nextNode(),a=0,s=0,l=i[0];for(;l!==void 0;){if(a===l.index){let c;l.type===2?c=new Ct(r,r.nextSibling,this,e):l.type===1?c=new l.ctor(r,l.name,l.strings,this,e):l.type===6&&(c=new gr(r,this,e)),this._$AV.push(c),l=i[++s]}a!==l?.index&&(r=j.nextNode(),a++)}return j.currentNode=Y,o}p(e){let n=0;for(const i of this._$AV)i!==void 0&&(i.strings!==void 0?(i._$AI(e,i,n),n+=i.strings.length-2):i._$AI(e[n])),n++}}class Ct{get _$AU(){return this._$AM?._$AU??this._$Cv}constructor(e,n,i,o){this.type=2,this._$AH=C,this._$AN=void 0,this._$AA=e,this._$AB=n,this._$AM=i,this.options=o,this._$Cv=o?.isConnected??!0}get parentNode(){let e=this._$AA.parentNode;const n=this._$AM;return n!==void 0&&e?.nodeType===11&&(e=n.parentNode),e}get startNode(){return this._$AA}get endNode(){return this._$AB}_$AI(e,n=this){e=tt(this,e,n),pt(e)?e===C||e==null||e===""?(this._$AH!==C&&this._$AR(),this._$AH=C):e!==this._$AH&&e!==E&&this._(e):e._$litType$!==void 0?this.$(e):e.nodeType!==void 0?this.T(e):cr(e)?this.k(e):this._(e)}O(e){return this._$AA.parentNode.insertBefore(e,this._$AB)}T(e){this._$AH!==e&&(this._$AR(),this._$AH=this.O(e))}_(e){this._$AH!==C&&pt(this._$AH)?this._$AA.nextSibling.data=e:this.T(Y.createTextNode(e)),this._$AH=e}$(e){const{values:n,_$litType$:i}=e,o=typeof i=="number"?this._$AC(e):(i.el===void 0&&(i.el=gt.createElement(ni(i.h,i.h[0]),this.options)),i);if(this._$AH?._$AD===o)this._$AH.p(n);else{const r=new hr(o,this),a=r.u(this.options);r.p(n),this.T(a),this._$AH=r}}_$AC(e){let n=dn.get(e.strings);return n===void 0&&dn.set(e.strings,n=new gt(e)),n}k(e){Me(this._$AH)||(this._$AH=[],this._$AR());const n=this._$AH;let i,o=0;for(const r of e)o===n.length?n.push(i=new Ct(this.O(mt()),this.O(mt()),this,this.options)):i=n[o],i._$AI(r),o++;o<n.length&&(this._$AR(i&&i._$AB.nextSibling,o),n.length=o)}_$AR(e=this._$AA.nextSibling,n){for(this._$AP?.(!1,!0,n);e!==this._$AB;){const i=on(e).nextSibling;on(e).remove(),e=i}}setConnected(e){this._$AM===void 0&&(this._$Cv=e,this._$AP?.(e))}}class Vt{get tagName(){return this.element.tagName}get _$AU(){return this._$AM._$AU}constructor(e,n,i,o,r){this.type=1,this._$AH=C,this._$AN=void 0,this.element=e,this.name=n,this._$AM=o,this.options=r,i.length>2||i[0]!==""||i[1]!==""?(this._$AH=Array(i.length-1).fill(new String),this.strings=i):this._$AH=C}_$AI(e,n=this,i,o){const r=this.strings;let a=!1;if(r===void 0)e=tt(this,e,n,0),a=!pt(e)||e!==this._$AH&&e!==E,a&&(this._$AH=e);else{const s=e;let l,c;for(e=r[0],l=0;l<r.length-1;l++)c=tt(this,s[i+l],n,l),c===E&&(c=this._$AH[l]),a||=!pt(c)||c!==this._$AH[l],c===C?e=C:e!==C&&(e+=(c??"")+r[l+1]),this._$AH[l]=c}a&&!o&&this.j(e)}j(e){e===C?this.element.removeAttribute(this.name):this.element.setAttribute(this.name,e??"")}}class fr extends Vt{constructor(){super(...arguments),this.type=3}j(e){this.element[this.name]=e===C?void 0:e}}class mr extends Vt{constructor(){super(...arguments),this.type=4}j(e){this.element.toggleAttribute(this.name,!!e&&e!==C)}}class pr extends Vt{constructor(e,n,i,o,r){super(e,n,i,o,r),this.type=5}_$AI(e,n=this){if((e=tt(this,e,n,0)??C)===E)return;const i=this._$AH,o=e===C&&i!==C||e.capture!==i.capture||e.once!==i.once||e.passive!==i.passive,r=e!==C&&(i===C||o);o&&this.element.removeEventListener(this.name,this,i),r&&this.element.addEventListener(this.name,this,e),this._$AH=e}handleEvent(e){typeof this._$AH=="function"?this._$AH.call(this.options?.host??this.element,e):this._$AH.handleEvent(e)}}class gr{constructor(e,n,i){this.element=e,this.type=6,this._$AN=void 0,this._$AM=n,this.options=i}get _$AU(){return this._$AM._$AU}_$AI(e){tt(this,e)}}const vr=_e.litHtmlPolyfillSupport;vr?.(gt,Ct),(_e.litHtmlVersions??=[]).push("3.3.3");const wr=(t,e,n)=>{const i=n?.renderBefore??e;let o=i._$litPart$;if(o===void 0){const r=n?.renderBefore??null;i._$litPart$=o=new Ct(e.insertBefore(mt(),r),r,void 0,n??{})}return o._$AI(t),o};const Te=globalThis;let ut=class extends J{constructor(){super(...arguments),this.renderOptions={host:this},this._$Do=void 0}createRenderRoot(){const e=super.createRenderRoot();return this.renderOptions.renderBefore??=e.firstChild,e}update(e){const n=this.render();this.hasUpdated||(this.renderOptions.isConnected=this.isConnected),super.update(e),this._$Do=wr(n,this.renderRoot,this.renderOptions)}connectedCallback(){super.connectedCallback(),this._$Do?.setConnected(!0)}disconnectedCallback(){super.disconnectedCallback(),this._$Do?.setConnected(!1)}render(){return E}};ut._$litElement$=!0,ut.finalized=!0,Te.litElementHydrateSupport?.({LitElement:ut});const br=Te.litElementPolyfillSupport;br?.({LitElement:ut});(Te.litElementVersions??=[]).push("4.2.2");const Lt=t=>(e,n)=>{n!==void 0?n.addInitializer(()=>{customElements.define(t,e)}):customElements.define(t,e)};const yr={attribute:!0,type:String,converter:Ot,reflect:!1,hasChanged:Ee},Cr=(t=yr,e,n)=>{const{kind:i,metadata:o}=n;let r=globalThis.litPropertyMetadata.get(o);if(r===void 0&&globalThis.litPropertyMetadata.set(o,r=new Map),i==="setter"&&((t=Object.create(t)).wrapped=!0),r.set(n.name,t),i==="accessor"){const{name:a}=n;return{set(s){const l=e.get.call(this);e.set.call(this,s),this.requestUpdate(a,l,t,!0,s)},init(s){return s!==void 0&&this.C(a,void 0,t,s),s}}}if(i==="setter"){const{name:a}=n;return function(s){const l=this[a];e.call(this,s),this.requestUpdate(a,l,t,!0,s)}}throw Error("Unsupported decorator location: "+i)};function h(t){return(e,n)=>typeof n=="object"?Cr(t,e,n):((i,o,r)=>{const a=o.hasOwnProperty(r);return o.constructor.createProperty(r,i),a?Object.getOwnPropertyDescriptor(o,r):void 0})(t,e,n)}function jt(t){return h({...t,state:!0,attribute:!1})}const Lr=(t,e,n)=>(n.configurable=!0,n.enumerable=!0,Reflect.decorate&&typeof e!="object"&&Object.defineProperty(t,e,n),n);function Wt(t,e){return(n,i,o)=>{const r=a=>a.renderRoot?.querySelector(t)??null;return Lr(n,i,{get(){return r(this)}})}}var xr=O`
  :host {
    box-sizing: border-box;
  }

  :host *,
  :host *::before,
  :host *::after {
    box-sizing: inherit;
  }

  [hidden],
  :host([hidden]) {
    display: none !important;
  }
`,Sr=/;\s+$/;function $r(t){return t.replace(/[A-Z]/g,e=>`-${e.toLowerCase()}`)}function un(t){const{property:e,value:n,element:i}=t;if(n){let o=i.getAttribute("style")||"";o&&(o.match(Sr)||(o+=";"),o+=" ");const r=`${e}: ${n}`;return o.includes(r)?void 0:`${o}${r};`}return null}var Mt,G=class extends ut{constructor(){super(),Go(this,Mt,!1),this.initialReflectedProperties=new Map,this.didSSR=!!this.shadowRoot,this.customStates={set:(e,n)=>{if(this.internals?.states)try{n?this.internals.states.add(e):this.internals.states.delete(e)}catch(i){if(String(i).includes("must start with '--'"))console.error("Your browser implements an outdated version of CustomStateSet. Consider using a polyfill");else throw i}},has:e=>{if(!this.internals?.states)return!1;try{return this.internals.states.has(e)}catch{return!1}}};try{this.internals=this.attachInternals()}catch{console.error("Element internals are not supported in your browser. Consider using a polyfill")}this.customStates.set("wa-defined",!0);let t=this.constructor;for(let[e,n]of t.elementProperties)n.default==="inherit"&&n.initial!==void 0&&typeof e=="string"&&this.customStates.set(`initial-${e}-${n.initial}`,!0)}static get styles(){const t=Array.isArray(this.css)?this.css:this.css?[this.css]:[];return[xr,...t]}connectedCallback(){super.connectedCallback(),this.didSSR||this.shadowRoot?.prepend(document.createComment(` Web Awesome: https://webawesome.com/docs/components/${this.localName.replace("wa-","")} `)),this.didSSR&&this.updateComplete.then(()=>{this.shadowRoot?.prepend(document.createComment(` Web Awesome: https://webawesome.com/docs/components/${this.localName.replace("wa-","")} `))})}attributeChangedCallback(t,e,n){Yo(this,Mt)||(this.constructor.elementProperties.forEach((i,o)=>{i.reflect&&this[o]!=null&&this.initialReflectedProperties.set(o,this[o])}),Jo(this,Mt,!0)),super.attributeChangedCallback(t,e,n)}willUpdate(t){super.willUpdate(t),this.initialReflectedProperties.forEach((e,n)=>{t.has(n)&&this[n]==null&&(this[n]=e)})}firstUpdated(t){super.firstUpdated(t),this.didSSR&&this.shadowRoot?.querySelectorAll("slot").forEach(e=>{e.dispatchEvent(new Event("slotchange",{bubbles:!0,composed:!1,cancelable:!1}))})}update(t){try{super.update(t)}catch(e){if(this.didSSR&&!this.hasUpdated){const n=new Event("lit-hydration-error",{bubbles:!0,composed:!0,cancelable:!1});n.error=e,this.dispatchEvent(n)}throw e}}setStyle(t,e){if(!this.style){const n=un({property:$r(t),value:e,element:this});n&&this.setAttribute("style",n);return}this.style[t]=e}setStyleProperty(t,e){if(!this.style){const n=un({property:t,value:e,element:this});n&&this.setAttribute("style",n);return}this.style.setProperty(t,e)}relayNativeEvent(t,e){t.stopImmediatePropagation(),this.dispatchEvent(new t.constructor(t.type,{...t,...e}))}};Mt=new WeakMap;d([h()],G.prototype,"dir",2);d([h()],G.prototype,"lang",2);d([h({type:Boolean,reflect:!0,attribute:"did-ssr"})],G.prototype,"didSSR",2);var kr=()=>({observedAttributes:["custom-error"],checkValidity(t){const e={message:"",isValid:!0,invalidKeys:[]};return t.customError&&(e.message=t.customError,e.isValid=!1,e.invalidKeys=["customError"]),e}}),_=class extends G{constructor(){super(),this.name=null,this.disabled=!1,this.required=!1,this.assumeInteractionOn=["input"],this.validators=[],this.valueHasChanged=!1,this.hasInteracted=!1,this.customError=null,this.emittedEvents=[],this.emitInvalid=t=>{t.target===this&&(this.hasInteracted=!0,this.dispatchEvent(new Yn))},this.handleInteraction=t=>{const e=this.emittedEvents;e.includes(t.type)||e.push(t.type),e.length===this.assumeInteractionOn?.length&&(this.hasInteracted=!0)},"addEventListener"in this&&this.addEventListener("invalid",this.emitInvalid)}static get validators(){return[kr()]}static get observedAttributes(){const t=new Set(super.observedAttributes||[]);for(const e of this.validators)if(e.observedAttributes)for(const n of e.observedAttributes)t.add(n);return[...t]}connectedCallback(){super.connectedCallback(),this.didSSR&&!this.hasUpdated?this.updateComplete.then(()=>{this.updateValidity()}):this.updateValidity(),this.assumeInteractionOn.forEach(t=>{this.addEventListener?.(t,this.handleInteraction)})}firstUpdated(...t){super.firstUpdated(...t),this.updateValidity()}willUpdate(t){if(t.has("customError")&&(this.customError||(this.customError=null),this.setCustomValidity(this.customError||"")),t.has("value")||t.has("disabled")||t.has("defaultValue")){const e=this.value;this.updateFormValue(e)}t.has("disabled")&&(this.customStates.set("disabled",this.disabled),(this.hasAttribute("disabled")||!this.matches(":disabled"))&&this.toggleAttribute("disabled",this.disabled)),super.willUpdate(t),this.didSSR&&!this.hasUpdated?this.updateComplete.then(()=>this.updateValidity()):this.updateValidity()}updateFormValue(t){if(Array.isArray(t)){if(this.name){const e=new FormData;for(const n of t)e.append(this.name,n);this.setValue(e,e)}}else this.setValue(t,t)}get labels(){return this.internals.labels}getForm(){return this.internals.form}set form(t){t?this.setAttribute("form",t):this.removeAttribute("form")}get form(){return this.internals.form}get validity(){return this.internals.validity}get willValidate(){return this.internals.willValidate}get validationMessage(){return this.internals.validationMessage}checkValidity(){return this.updateValidity(),this.internals.checkValidity()}reportValidity(){return this.updateValidity(),this.hasInteracted=!0,this.internals.reportValidity()}get validationTarget(){return this.input||void 0}setValidity(...t){const e=t[0],n=t[1];let i=t[2];i||(i=this.validationTarget),this.internals.setValidity(e,n,i||void 0),this.requestUpdate("validity"),this.setCustomStates()}setCustomStates(){const t=!!this.required,e=this.internals.validity.valid,n=this.hasInteracted;this.customStates.set("required",t),this.customStates.set("optional",!t),this.customStates.set("invalid",!e),this.customStates.set("valid",e),this.customStates.set("user-invalid",!e&&n),this.customStates.set("user-valid",e&&n)}setCustomValidity(t){if(!t){this.customError=null,this.setValidity({});return}this.customError=t,this.setValidity({customError:!0},t,this.validationTarget)}formResetCallback(){this.resetValidity(),this.hasInteracted=!1,this.valueHasChanged=!1,this.emittedEvents=[],this.updateValidity()}formDisabledCallback(t){this.disabled=t,this.updateValidity()}formStateRestoreCallback(t,e){this.didSSR&&!this.hasUpdated?this.updateComplete.then(()=>{this.value=t,e==="restore"&&this.resetValidity(),this.updateValidity()}):(this.value=t,e==="restore"&&this.resetValidity(),this.updateValidity())}setValue(...t){const[e,n]=t;this.internals.setFormValue(e,n)}get allValidators(){const t=this.constructor.validators||[],e=this.validators||[];return[...t,...e]}resetValidity(){this.setCustomValidity(""),this.setValidity({})}updateValidity(){if(this.disabled||this.hasAttribute("disabled")||!this.willValidate){this.resetValidity();return}const t=this.allValidators;if(!t?.length)return;const e={customError:!!this.customError},n=this.validationTarget||this.input||void 0;let i="";for(const o of t){const{isValid:r,message:a,invalidKeys:s}=o.checkValidity(this);r||(i||(i=a),s?.length>=0&&s.forEach(l=>e[l]=!0))}i||(i=this.validationMessage),this.setValidity(e,i,n)}};_.formAssociated=!0;d([h({reflect:!0})],_.prototype,"name",2);d([h({type:Boolean})],_.prototype,"disabled",2);d([h({state:!0,attribute:!1})],_.prototype,"valueHasChanged",2);d([h({state:!0,attribute:!1})],_.prototype,"hasInteracted",2);d([h({attribute:"custom-error",reflect:!0})],_.prototype,"customError",2);d([h({attribute:!1,state:!0,type:Object})],_.prototype,"validity",1);var Fe=class{constructor(t,...e){this.slotNames=[],this.handleSlotChange=n=>{const i=n.target;(this.slotNames.includes("[default]")&&!i.name||i.name&&this.slotNames.includes(i.name))&&this.host.requestUpdate()},(this.host=t).addController(this),this.slotNames=e}hasDefaultSlot(){return this.host.childNodes?[...this.host.childNodes].some(t=>{if(t.nodeType===Node.TEXT_NODE&&t.textContent.trim()!=="")return!0;if(t.nodeType===Node.ELEMENT_NODE){const e=t;if(e.tagName.toLowerCase()==="wa-visually-hidden")return!1;if(!e.hasAttribute("slot"))return!0}return!1}):!1}hasNamedSlot(t){return this.host.querySelector?.(`:scope > [slot="${t}"]`)!==null}test(t,e){return e&&this.host.didSSR&&!this.host.hasUpdated?!!this.host[e]:t==="[default]"?this.hasDefaultSlot():this.hasNamedSlot(t)}hostConnected(){const t=this.host.shadowRoot;t&&"addEventListener"in t&&t.addEventListener("slotchange",this.handleSlotChange)}hostDisconnected(){const t=this.host.shadowRoot;t&&"removeEventListener"in t&&t.removeEventListener("slotchange",this.handleSlotChange)}};var Ar=O`
  @layer wa-component {
    :host {
      display: inline-block;

      /* Workaround because Chrome doesn't like :host(:has()) below
       * https://issues.chromium.org/issues/40062355
       * Firefox doesn't like this nested rule, so both are needed */
      &:has(wa-badge) {
        position: relative;
      }
    }

    /* Apply relative positioning only when needed to position wa-badge
     * This avoids creating a new stacking context for every button */
    :host(:has(wa-badge)) {
      position: relative;
    }
  }

  .button {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    text-decoration: none;
    user-select: none;
    -webkit-user-select: none;
    white-space: nowrap;
    vertical-align: middle;
    transition-property: background, border, box-shadow, color, opacity, transform;
    transition-duration: var(--wa-transition-fast);
    transition-timing-function: var(--wa-transition-easing);
    transform-origin: center;
    cursor: pointer;
    padding: 0 var(--wa-form-control-padding-inline);
    font-family: inherit;
    font-size: inherit;
    font-weight: var(--wa-font-weight-action);
    height: var(--wa-form-control-height);
    width: 100%;

    background-color: var(--wa-color-fill-loud, var(--wa-color-neutral-fill-loud));

    border-color: transparent;
    color: var(--wa-color-on-loud, var(--wa-color-neutral-on-loud));
    border-start-start-radius: var(--_button-start-start-radius, var(--wa-form-control-border-radius));
    border-start-end-radius: var(--_button-start-end-radius, var(--wa-form-control-border-radius));
    border-end-start-radius: var(--_button-end-start-radius, var(--wa-form-control-border-radius));
    border-end-end-radius: var(--_button-end-end-radius, var(--wa-form-control-border-radius));
    border-style: var(--wa-form-control-border-style);
    border-width: var(--wa-form-control-border-width);
  }

  /* Hover and active transforms */
  .button:not(.disabled):not(.loading) {
    @media (hover: hover) {
      &:hover {
        transform: var(--wa-button-transform-hover);
      }
    }
    &:active {
      transform: var(--wa-button-transform-active);
    }

    @media (prefers-reduced-motion: reduce) {
      &:hover,
      &:active {
        transform: none;
      }
    }
  }

  /* Appearance modifiers */
  :host([appearance='plain']) {
    /* Indentation overrides for grouping */
    margin-inline-start: var(--_button-horizontal-indent);
    margin-block-start: var(--_button-vertical-indent);

    .button {
      color: var(--wa-color-on-quiet, var(--wa-color-neutral-on-quiet));
      background-color: transparent;
      border-color: transparent;
    }
    @media (hover: hover) {
      .button:not(.disabled):not(.loading):hover {
        color: var(--wa-color-on-quiet, var(--wa-color-neutral-on-quiet));
        background-color: var(--wa-color-fill-quiet, var(--wa-color-neutral-fill-quiet));
      }
    }
    .button:not(.disabled):not(.loading):active {
      color: var(--wa-color-on-quiet, var(--wa-color-neutral-on-quiet));
      background-color: color-mix(
        in oklab,
        var(--wa-color-fill-quiet, var(--wa-color-neutral-fill-quiet)),
        var(--wa-color-mix-active)
      );
    }
  }

  :host([appearance='outlined']) {
    /* Indentation overrides for grouping outlined */
    margin-inline-start: var(--_button-horizontal-indent-outlined);
    margin-block-start: var(--_button-vertical-indent-outlined);

    .button {
      color: var(--wa-color-on-quiet, var(--wa-color-neutral-on-quiet));
      background-color: transparent;
      border-color: var(--wa-color-border-loud, var(--wa-color-neutral-border-loud));
    }
    @media (hover: hover) {
      .button:not(.disabled):not(.loading):hover {
        color: var(--wa-color-on-quiet, var(--wa-color-neutral-on-quiet));
        background-color: var(--wa-color-fill-quiet, var(--wa-color-neutral-fill-quiet));
      }
    }
    .button:not(.disabled):not(.loading):active {
      color: var(--wa-color-on-quiet, var(--wa-color-neutral-on-quiet));
      background-color: color-mix(
        in oklab,
        var(--wa-color-fill-quiet, var(--wa-color-neutral-fill-quiet)),
        var(--wa-color-mix-active)
      );
    }
  }

  :host([appearance='filled']) {
    /* Indentation overrides for grouping */
    margin-inline-start: var(--_button-horizontal-indent);
    margin-block-start: var(--_button-vertical-indent);

    .button {
      color: var(--wa-color-on-normal, var(--wa-color-neutral-on-normal));
      background-color: var(--wa-color-fill-normal, var(--wa-color-neutral-fill-normal));
      border-color: transparent;
    }
    @media (hover: hover) {
      .button:not(.disabled):not(.loading):hover {
        color: var(--wa-color-on-normal, var(--wa-color-neutral-on-normal));
        background-color: color-mix(
          in oklab,
          var(--wa-color-fill-normal, var(--wa-color-neutral-fill-normal)),
          var(--wa-color-mix-hover)
        );
      }
    }
    .button:not(.disabled):not(.loading):active {
      color: var(--wa-color-on-normal, var(--wa-color-neutral-on-normal));
      background-color: color-mix(
        in oklab,
        var(--wa-color-fill-normal, var(--wa-color-neutral-fill-normal)),
        var(--wa-color-mix-active)
      );
    }
  }

  :host([appearance='filled-outlined']) {
    /* Indentation overrides for grouping outlined */
    margin-inline-start: var(--_button-horizontal-indent-outlined);
    margin-block-start: var(--_button-vertical-indent-outlined);

    .button {
      color: var(--wa-color-on-normal, var(--wa-color-neutral-on-normal));
      background-color: var(--wa-color-fill-normal, var(--wa-color-neutral-fill-normal));
      border-color: var(--wa-color-border-normal, var(--wa-color-neutral-border-normal));
    }
    @media (hover: hover) {
      .button:not(.disabled):not(.loading):hover {
        color: var(--wa-color-on-normal, var(--wa-color-neutral-on-normal));
        background-color: color-mix(
          in oklab,
          var(--wa-color-fill-normal, var(--wa-color-neutral-fill-normal)),
          var(--wa-color-mix-hover)
        );
      }
    }
    .button:not(.disabled):not(.loading):active {
      color: var(--wa-color-on-normal, var(--wa-color-neutral-on-normal));
      background-color: color-mix(
        in oklab,
        var(--wa-color-fill-normal, var(--wa-color-neutral-fill-normal)),
        var(--wa-color-mix-active)
      );
    }
  }

  :host([appearance='accent']) {
    /* Indentation overrides for grouping */
    margin-inline-start: var(--_button-horizontal-indent);
    margin-block-start: var(--_button-vertical-indent);

    .button {
      color: var(--wa-color-on-loud, var(--wa-color-neutral-on-loud));
      background-color: var(--wa-color-fill-loud, var(--wa-color-neutral-fill-loud));
      border-color: transparent;
    }
    @media (hover: hover) {
      .button:not(.disabled):not(.loading):hover {
        background-color: color-mix(
          in oklab,
          var(--wa-color-fill-loud, var(--wa-color-neutral-fill-loud)),
          var(--wa-color-mix-hover)
        );
      }
    }
    .button:not(.disabled):not(.loading):active {
      background-color: color-mix(
        in oklab,
        var(--wa-color-fill-loud, var(--wa-color-neutral-fill-loud)),
        var(--wa-color-mix-active)
      );
    }
  }

  /* Focus states */
  .button:focus {
    outline: none;
  }

  .button:focus-visible {
    outline: var(--wa-focus-ring);
    outline-offset: var(--wa-focus-ring-offset);
  }

  /* Disabled state */
  :host([disabled]) {
    opacity: 0.5;
    cursor: not-allowed;

    /* When disabled, prevent mouse events from bubbling up from children */
    .button {
      pointer-events: none;
    }
  }

  /* Keep it last so Safari doesn't stop parsing this block */
  .button::-moz-focus-inner {
    border: 0;
  }

  /* Icon buttons */
  .button.is-icon-button {
    outline-offset: 2px;
    width: var(--wa-form-control-height);
    aspect-ratio: 1;
  }

  /* Icon buttons with a caret need to grow to fit both the icon and the caret */
  .button.is-icon-button.caret {
    width: auto;
    aspect-ratio: auto;
    min-width: var(--wa-form-control-height);
  }

  /* Pill modifier */
  :host([pill]) .button {
    border-start-start-radius: var(--_button-start-start-radius, var(--wa-border-radius-pill));
    border-start-end-radius: var(--_button-start-end-radius, var(--wa-border-radius-pill));
    border-end-start-radius: var(--_button-end-start-radius, var(--wa-border-radius-pill));
    border-end-end-radius: var(--_button-end-end-radius, var(--wa-border-radius-pill));
  }

  /*
   * Label
   */

  .start,
  .end {
    flex: 0 0 auto;
    display: flex;
    align-items: center;
    pointer-events: none;
  }

  .label {
    display: inline-block;
  }

  .is-icon-button .label {
    display: flex;
    justify-content: center;
  }

  .label::slotted(wa-icon) {
    align-self: center;
  }

  /*
   * Caret modifier
   */

  wa-icon[part='caret'] {
    display: flex;
    align-self: center;
    align-items: center;

    &::part(svg) {
      width: 0.875em;
      height: 0.875em;
    }

    .button:has(&) .end {
      display: none;
    }
  }

  /*
   * Loading modifier
   */

  .loading {
    position: relative;
    cursor: wait;

    .start,
    .label,
    .end,
    .caret {
      visibility: hidden;
    }

    wa-spinner {
      --indicator-color: currentColor;
      --track-color: color-mix(in oklab, currentColor, transparent 90%);

      position: absolute;
      font-size: 1em;
      height: 1em;
      width: 1em;
      top: calc(50% - 0.5em);
      left: calc(50% - 0.5em);
    }
  }

  /*
   * Badges
   */

  .button ::slotted(wa-badge) {
    border-color: var(--wa-color-surface-default);
    position: absolute;
    inset-block-start: 0;
    inset-inline-end: 0;
    translate: 50% -50%;
    pointer-events: none;
  }

  :host(:dir(rtl)) ::slotted(wa-badge) {
    translate: -50% -50%;
  }

  /*
  * Button spacing
  */

  slot[name='start']::slotted(*) {
    margin-inline-end: 0.75em;
  }

  slot[name='end']::slotted(*),
  .button:not(.visually-hidden-label) [part='caret'] {
    margin-inline-start: 0.75em;
  }
`;var hn={small:"s",medium:"m",large:"l"},fn=new Set;function ii(t,e){e in hn&&!fn.has(`${t}:${e}`)&&(fn.add(`${t}:${e}`),console.warn(`[${t}] size="${e}" is deprecated. Use size="${hn[e]}" instead. The long-form value will be removed in the next major version.`))}var oi=O`
  :host([size='xs']) {
    font-size: var(--wa-font-size-xs);
  }

  :host([size='s']),
  :host([size='small']) {
    font-size: var(--wa-font-size-s);
  }

  :host([size='m']),
  :host([size='medium']) {
    font-size: var(--wa-font-size-m);
  }

  :host([size='l']),
  :host([size='large']) {
    font-size: var(--wa-font-size-l);
  }

  :host([size='xl']) {
    font-size: var(--wa-font-size-xl);
  }
`;var zr=O`
  :where(:root),
  .wa-neutral,
  :host([variant='neutral']) {
    --wa-color-fill-loud: var(--wa-color-neutral-fill-loud);
    --wa-color-fill-normal: var(--wa-color-neutral-fill-normal);
    --wa-color-fill-quiet: var(--wa-color-neutral-fill-quiet);
    --wa-color-border-loud: var(--wa-color-neutral-border-loud);
    --wa-color-border-normal: var(--wa-color-neutral-border-normal);
    --wa-color-border-quiet: var(--wa-color-neutral-border-quiet);
    --wa-color-on-loud: var(--wa-color-neutral-on-loud);
    --wa-color-on-normal: var(--wa-color-neutral-on-normal);
    --wa-color-on-quiet: var(--wa-color-neutral-on-quiet);
  }

  .wa-brand,
  :host([variant='brand']) {
    --wa-color-fill-loud: var(--wa-color-brand-fill-loud);
    --wa-color-fill-normal: var(--wa-color-brand-fill-normal);
    --wa-color-fill-quiet: var(--wa-color-brand-fill-quiet);
    --wa-color-border-loud: var(--wa-color-brand-border-loud);
    --wa-color-border-normal: var(--wa-color-brand-border-normal);
    --wa-color-border-quiet: var(--wa-color-brand-border-quiet);
    --wa-color-on-loud: var(--wa-color-brand-on-loud);
    --wa-color-on-normal: var(--wa-color-brand-on-normal);
    --wa-color-on-quiet: var(--wa-color-brand-on-quiet);
  }

  .wa-success,
  :host([variant='success']) {
    --wa-color-fill-loud: var(--wa-color-success-fill-loud);
    --wa-color-fill-normal: var(--wa-color-success-fill-normal);
    --wa-color-fill-quiet: var(--wa-color-success-fill-quiet);
    --wa-color-border-loud: var(--wa-color-success-border-loud);
    --wa-color-border-normal: var(--wa-color-success-border-normal);
    --wa-color-border-quiet: var(--wa-color-success-border-quiet);
    --wa-color-on-loud: var(--wa-color-success-on-loud);
    --wa-color-on-normal: var(--wa-color-success-on-normal);
    --wa-color-on-quiet: var(--wa-color-success-on-quiet);
  }

  .wa-warning,
  :host([variant='warning']) {
    --wa-color-fill-loud: var(--wa-color-warning-fill-loud);
    --wa-color-fill-normal: var(--wa-color-warning-fill-normal);
    --wa-color-fill-quiet: var(--wa-color-warning-fill-quiet);
    --wa-color-border-loud: var(--wa-color-warning-border-loud);
    --wa-color-border-normal: var(--wa-color-warning-border-normal);
    --wa-color-border-quiet: var(--wa-color-warning-border-quiet);
    --wa-color-on-loud: var(--wa-color-warning-on-loud);
    --wa-color-on-normal: var(--wa-color-warning-on-normal);
    --wa-color-on-quiet: var(--wa-color-warning-on-quiet);
  }

  .wa-danger,
  :host([variant='danger']) {
    --wa-color-fill-loud: var(--wa-color-danger-fill-loud);
    --wa-color-fill-normal: var(--wa-color-danger-fill-normal);
    --wa-color-fill-quiet: var(--wa-color-danger-fill-quiet);
    --wa-color-border-loud: var(--wa-color-danger-border-loud);
    --wa-color-border-normal: var(--wa-color-danger-border-normal);
    --wa-color-border-quiet: var(--wa-color-danger-border-quiet);
    --wa-color-on-loud: var(--wa-color-danger-on-loud);
    --wa-color-on-normal: var(--wa-color-danger-on-normal);
    --wa-color-on-quiet: var(--wa-color-danger-on-quiet);
  }
`;function D(t,e){const n={waitUntilFirstUpdate:!1,...e};return(i,o)=>{const{update:r}=i,a=Array.isArray(t)?t:[t];i.update=function(s){a.forEach(l=>{const c=l;if(s.has(c)){const u=s.get(c),f=this[c];u!==f&&(!n.waitUntilFirstUpdate||this.hasUpdated)&&this[o](u,f)}}),r.call(this,s)}}}const oe=new Set,Z=new Map;let R,Ie="ltr",Re="en";const ri=typeof MutationObserver<"u"&&typeof document<"u"&&typeof document.documentElement<"u";if(ri){const t=new MutationObserver(si);Ie=document.documentElement.dir||"ltr",Re=document.documentElement.lang||navigator.language,t.observe(document.documentElement,{attributes:!0,attributeFilter:["dir","lang"]})}function ai(...t){t.map(e=>{const n=e.$code.toLowerCase();Z.has(n)?Z.set(n,Object.assign(Object.assign({},Z.get(n)),e)):Z.set(n,e),R||(R=e)}),si()}function si(){ri&&(Ie=document.documentElement.dir||"ltr",Re=document.documentElement.lang||navigator.language),[...oe.keys()].map(t=>{typeof t.requestUpdate=="function"&&t.requestUpdate()})}let Er=class{constructor(e){this.host=e,this.host.addController(this)}hostConnected(){oe.add(this.host)}hostDisconnected(){oe.delete(this.host)}dir(){return`${this.host.dir||Ie}`.toLowerCase()}lang(){const e=`${this.host.lang||Re}`.toLowerCase().replace(/_/g,"-");try{return new Intl.Locale(e),e}catch{return R?R.$code.toLowerCase():"en"}}getTranslationData(e){var n,i;let o;try{o=new Intl.Locale(e.replace(/_/g,"-"))}catch{return{locale:void 0,language:"",region:"",primary:void 0,secondary:void 0}}const r=o.language.toLowerCase(),a=(i=(n=o.region)===null||n===void 0?void 0:n.toLowerCase())!==null&&i!==void 0?i:"",s=Z.get(`${r}-${a}`),l=Z.get(r);return{locale:o,language:r,region:a,primary:s,secondary:l}}exists(e,n){var i;const{primary:o,secondary:r}=this.getTranslationData((i=n.lang)!==null&&i!==void 0?i:this.lang());return n=Object.assign({includeFallback:!1},n),!!(o&&o[e]||r&&r[e]||n.includeFallback&&R&&R[e])}term(e,...n){const{primary:i,secondary:o}=this.getTranslationData(this.lang());let r;if(i&&i[e])r=i[e];else if(o&&o[e])r=o[e];else if(R&&R[e])r=R[e];else return console.error(`No translation found for: ${String(e)}`),String(e);return typeof r=="function"?r(...n):r}date(e,n){return e=new Date(e),new Intl.DateTimeFormat(this.lang(),n).format(e)}number(e,n){return e=Number(e),isNaN(e)?"":new Intl.NumberFormat(this.lang(),n).format(e)}relativeTime(e,n,i){return new Intl.RelativeTimeFormat(this.lang(),i).format(e,n)}};var li={$code:"en",$name:"English",$dir:"ltr",am:"AM",autosizeColumn:"Autosize column",captions:"Captions",carousel:"Carousel",chooseDate:"Choose date",chooseDecade:"Choose decade",chooseMonth:"Choose month",chooseTime:"Choose time",chooseYear:"Choose year",clearEntry:"Clear entry",clearFilter:"Clear filter",clearSort:"Clear sort",close:"Close",closeCalendar:"Close calendar",closeTimeInput:"Close time picker",collapseRow:"Collapse row",columnMenu:"Column options",columnMovedToPosition:(t,e,n)=>`${t} moved to position ${e} of ${n}`,columns:"Columns",compactPageXOfY:(t,e)=>`${t} of ${e}`,copied:"Copied",copy:"Copy",createOption:t=>`Create "${t}"`,currentlyPlaying:"currently playing",currentValue:"Current value",date:"Date",datePickerKeyboardHelp:"Use arrow keys to change values; press Alt+Down Arrow to open the calendar.",day:"Day",dayPeriod:"AM/PM",decrement:"Decrement",deselectAllRows:"Deselect all rows",dropFileHere:"Drop file here or click to browse",dropFilesHere:"Drop files here or click to browse",empty:"Empty",endDate:"End date",enterFullscreen:"Enter fullscreen",error:"Error",exitFullscreen:"Exit fullscreen",expandRow:"Expand row",filterByColumn:t=>`Filter by ${t}`,filterFrom:"From",filterMax:"Max",filterMin:"Min",filterTo:"To",firstPage:"First page",goToSlide:(t,e)=>`Go to slide ${t} of ${e}`,hideColumn:"Hide column",hidePassword:"Hide password",hour:"Hour",incompleteDate:"Enter a valid date.",increment:"Increment",jumpBackwardX:t=>`Jump back ${t} pages`,jumpForwardX:t=>`Jump forward ${t} pages`,lastPage:"Last page",loading:"Loading",minute:"Minute",month:"Month",moreOptions:"More Options",mute:"Mute",nextDecade:"Next decade",nextMonth:"Next month",nextPage:"Next page",nextSlide:"Next slide",nextVideo:"Next Video",nextYear:"Next year",noData:"No data",noResults:"No matching results",now:"Now",numCharacters:t=>t===1?"1 character":`${t} characters`,numCharactersRemaining:t=>t===1?"1 character remaining":`${t} characters remaining`,numOptionsSelected:t=>t===0?"No options selected":t===1?"1 option selected":`${t} options selected`,numRowsCopied:t=>t===1?"1 row copied":`${t} rows copied`,numRowsSelected:t=>t===1?"1 row selected":`${t} rows selected`,pageXOfY:(t,e)=>`Page ${t} of ${e}`,pagination:"Pagination",pause:"Pause",pauseAnimation:"Pause animation",pictureInPicture:"Picture in picture",pinLeft:"Pin left",pinRight:"Pin right",play:"Play",playAnimation:"Play animation",playbackSpeed:"Playback speed",playlist:"Playlist",pm:"PM",previousDecade:"Previous decade",previousMonth:"Previous month",previousPage:"Previous page",previousSlide:"Previous slide",previousVideo:"Previous video",previousYear:"Previous year",progress:"Progress",rangeTooLong:t=>t===1?"Select a range no longer than 1 day":`Select a range no longer than ${t} days`,rangeTooShort:t=>t===1?"Select a range at least 1 day long":`Select a range at least ${t} days long`,readonly:"Read-only",remove:"Remove",resetColumns:"Reset columns",resize:"Resize",resizeColumn:"Resize column",rowsPerPage:"Rows per page",scrollableRegion:"Scrollable region",scrollToEnd:"Scroll to end",scrollToStart:"Scroll to start",search:"Search",second:"Second",seek:"Seek",seekProgress:(t,e)=>`${t} of ${e}`,selectAColorFromTheScreen:"Select a color from the screen",selectAllRows:"Select all rows",selected:"Selected",selectedDateLabel:t=>`Selected: ${t}`,selectedRangeLabel:t=>`Selected range: ${t}`,selectGroup:"Select group",selectionCleared:"Selection cleared",selectRow:"Select row",showingNofMRows:(t,e)=>`Showing ${t} of ${e} rows`,showingXtoYofZ:(t,e,n)=>`${t}–${e} of ${n}`,showPassword:"Show password",slideNum:t=>`Slide ${t}`,sortAscending:"Sort ascending",sortColumn:"Sort column",sortDescending:"Sort descending",startDate:"Start date",time:"Time",timeInputKeyboardHelp:"Use arrow keys to change values; press Alt+Down Arrow to open the time picker.",today:"Today",toggleColorFormat:"Toggle color format",unmute:"Unmute",unpin:"Unpin",unpinColumn:"Unpin column",videoPlayer:"Video player",volume:"Volume",year:"Year",zoomIn:"Zoom in",zoomOut:"Zoom out"};ai(li);var _r=li;var Ht=class extends Er{lang(){return this.host.didSSR&&!this.host.hasUpdated?this.host.lang||"en":super.lang()}};ai(_r);const V={ATTRIBUTE:1,PROPERTY:3,BOOLEAN_ATTRIBUTE:4},ci=t=>(...e)=>({_$litDirective$:t,values:e});let di=class{constructor(e){}get _$AU(){return this._$AM._$AU}_$AT(e,n,i){this._$Ct=e,this._$AM=n,this._$Ci=i}_$AS(e,n){return this.update(e,n)}update(e,n){return this.render(...n)}};const Bt=ci(class extends di{constructor(t){if(super(t),t.type!==V.ATTRIBUTE||t.name!=="class"||t.strings?.length>2)throw Error("`classMap()` can only be used in the `class` attribute and must be the only part in the attribute.")}render(t){return" "+Object.keys(t).filter(e=>t[e]).join(" ")+" "}update(t,[e]){if(this.st===void 0){this.st=new Set,t.strings!==void 0&&(this.nt=new Set(t.strings.join(" ").split(/\s/).filter(i=>i!=="")));for(const i in e)e[i]&&!this.nt?.has(i)&&this.st.add(i);return this.render(e)}const n=t.element.classList;for(const i of this.st)i in e||(n.remove(i),this.st.delete(i));for(const i in e){const o=!!e[i];o===this.st.has(i)||this.nt?.has(i)||(o?(n.add(i),this.st.add(i)):(n.remove(i),this.st.delete(i)))}return E}});const y=t=>t??C;const ui=Symbol.for(""),Mr=t=>{if(t?.r===ui)return t?._$litStatic$},mn=(t,...e)=>({_$litStatic$:e.reduce((n,i,o)=>n+(r=>{if(r._$litStatic$!==void 0)return r._$litStatic$;throw Error(`Value passed to 'literal' function must be a 'literal' result: ${r}. Use 'unsafeStatic' to pass non-literal values, but
            take care to ensure page security.`)})(i)+t[o+1],t[0]),r:ui}),pn=new Map,Tr=t=>(e,...n)=>{const i=n.length;let o,r;const a=[],s=[];let l,c=0,u=!1;for(;c<i;){for(l=e[c];c<i&&(r=n[c],(o=Mr(r))!==void 0);)l+=o+e[++c],u=!0;c!==i&&s.push(r),a.push(l),c++}if(c===i&&a.push(e[i]),u){const f=a.join("$$lit$$");(e=pn.get(f))===void 0&&(a.raw=a,pn.set(f,e=a)),n=s}return t(e,...n)},Yt=Tr(z);var g=class extends _{constructor(){super(...arguments),this.assumeInteractionOn=["click"],this.hasSlotController=new Fe(this,"[default]","start","end"),this.localize=new Ht(this),this.invalid=!1,this.isIconButton=!1,this.title="",this.variant="neutral",this.appearance="accent",this.size="m",this.withCaret=!1,this.withStart=!1,this.withEnd=!1,this.disabled=!1,this.loading=!1,this.pill=!1,this.type="button"}static get validators(){return[...super.validators,Kn()]}handleSizeChange(){ii(this.localName,this.size)}constructLightDOMButton(){const t=document.createElement("button");for(const e of this.attributes)e.name!=="style"&&t.setAttribute(e.name,e.value);return t.type=this.type,t.style.position="absolute !important",t.style.width="0 !important",t.style.height="0 !important",t.style.clipPath="inset(50%) !important",t.style.overflow="hidden !important",t.style.whiteSpace="nowrap !important",this.name&&(t.name=this.name),t.value=this.value||"",t}handleClick(t){if(this.disabled||this.loading){t.preventDefault(),t.stopImmediatePropagation();return}if(this.type!=="submit"&&this.type!=="reset"||!this.getForm())return;const n=this.constructLightDOMButton();this.parentElement?.append(n),n.click(),n.remove()}handleInvalid(){this.dispatchEvent(new Yn)}handleLabelSlotChange(){const t=this.labelSlot.assignedNodes({flatten:!0});let e=!1,n=!1,i=!1,o=!1;[...t].forEach(r=>{if(r.nodeType===Node.ELEMENT_NODE){const a=r;a.localName==="wa-icon"?(n=!0,e||(e=a.label!==void 0)):o=!0}else r.nodeType===Node.TEXT_NODE&&(r.textContent?.trim()||"").length>0&&(i=!0)}),this.isIconButton=n&&!i&&!o,this.customStates.set("icon-button",this.isIconButton),this.isIconButton&&!e&&console.warn('Icon buttons must have a label for screen readers. Add <wa-icon label="..."> to remove this warning.',this)}isButton(){return!this.href}isLink(){return!!this.href}handleDisabledChange(){this.customStates.set("disabled",this.disabled),this.updateValidity()}handleHrefChange(){this.customStates.set("link",this.isLink())}handleLoadingChange(){this.customStates.set("loading",this.loading)}setValue(...t){}click(){this.button.click()}focus(t){this.button.focus(t)}blur(){this.button.blur()}render(){const t=this.isLink(),e=t?mn`a`:mn`button`;return Yt`
      <${e}
        part="base button"
        class=${Bt({button:!0,caret:this.withCaret,disabled:this.disabled,loading:this.loading,rtl:this.localize.dir()==="rtl","has-label":this.hasSlotController.test("[default]"),"has-start":this.hasSlotController.test("start","withStart"),"has-end":this.hasSlotController.test("end","withEnd"),"is-icon-button":this.isIconButton})}
        ?disabled=${y(t?void 0:this.disabled)}
        type=${y(t?void 0:this.type)}
        title=${this.title}
        name=${y(t?void 0:this.name)}
        value=${y(t?void 0:this.value)}
        href=${y(t?this.href:void 0)}
        target=${y(t?this.target:void 0)}
        download=${y(t?this.download:void 0)}
        rel=${y(t&&this.rel?this.rel:void 0)}
        role=${y(t?void 0:"button")}
        aria-disabled=${y(t&&this.disabled?"true":void 0)}
        tabindex=${this.disabled?"-1":"0"}
        @invalid=${this.isButton()?this.handleInvalid:null}
        @click=${this.handleClick}
      >
        <slot name="start" part="start" class="start"></slot>
        <slot part="label" class="label" @slotchange=${this.handleLabelSlotChange}></slot>
        <slot name="end" part="end" class="end"></slot>
        ${this.withCaret?Yt`
                <wa-icon part="caret" class="caret" library="system" name="chevron-down" variant="solid"></wa-icon>
              `:""}
        ${this.loading?Yt`<wa-spinner part="spinner"></wa-spinner>`:""}
      </${e}>
    `}};g.shadowRootOptions={..._.shadowRootOptions,delegatesFocus:!0};g.css=[Ar,zr,oi];d([Wt(".button")],g.prototype,"button",2);d([Wt("slot:not([name])")],g.prototype,"labelSlot",2);d([jt()],g.prototype,"invalid",2);d([jt()],g.prototype,"isIconButton",2);d([h()],g.prototype,"title",2);d([h({reflect:!0})],g.prototype,"variant",2);d([h({reflect:!0})],g.prototype,"appearance",2);d([h({reflect:!0})],g.prototype,"size",2);d([D("size")],g.prototype,"handleSizeChange",1);d([h({attribute:"with-caret",type:Boolean,reflect:!0})],g.prototype,"withCaret",2);d([h({attribute:"with-start",type:Boolean})],g.prototype,"withStart",2);d([h({attribute:"with-end",type:Boolean})],g.prototype,"withEnd",2);d([h({type:Boolean})],g.prototype,"disabled",2);d([h({type:Boolean,reflect:!0})],g.prototype,"loading",2);d([h({type:Boolean,reflect:!0})],g.prototype,"pill",2);d([h()],g.prototype,"type",2);d([h({reflect:!0})],g.prototype,"name",2);d([h({reflect:!0})],g.prototype,"value",2);d([h({reflect:!0})],g.prototype,"href",2);d([h()],g.prototype,"target",2);d([h()],g.prototype,"rel",2);d([h()],g.prototype,"download",2);d([h({attribute:"formaction"})],g.prototype,"formAction",2);d([h({attribute:"formenctype"})],g.prototype,"formEnctype",2);d([h({attribute:"formmethod"})],g.prototype,"formMethod",2);d([h({attribute:"formnovalidate",type:Boolean})],g.prototype,"formNoValidate",2);d([h({attribute:"formtarget"})],g.prototype,"formTarget",2);d([D("disabled",{waitUntilFirstUpdate:!0})],g.prototype,"handleDisabledChange",1);d([D("href")],g.prototype,"handleHrefChange",1);d([D("loading",{waitUntilFirstUpdate:!0})],g.prototype,"handleLoadingChange",1);g=d([Lt("wa-button")],g);g.disableWarning?.("change-in-update");var Fr=O`
  :host {
    --track-width: 2px;
    --track-color: var(--wa-color-neutral-fill-normal);
    --indicator-color: var(--wa-color-brand-fill-loud);
    --speed: 2s;
    --size: 1em;

    /*
      Resizing a spinner element using anything but font-size will break the animation because the animation uses em
      units. Therefore, if a spinner is used in a flex container without \`flex: none\` applied, the spinner can
      grow/shrink and break the animation. The use of \`flex: none\` on the host element prevents this by always having
      the spinner sized according to its actual dimensions.
    */
    flex: none;
    display: inline-flex;
    width: var(--size);
    height: var(--size);
  }

  svg {
    width: 100%;
    height: 100%;
    aspect-ratio: 1;
    animation: spin var(--speed) linear infinite;
  }

  .track,
  .indicator {
    --radius: calc(var(--size) / 2 - var(--track-width) / 2);
    --circumference: calc(var(--radius) * 2 * 3.141592654);

    cx: calc(var(--size) / 2);
    cy: calc(var(--size) / 2);
    r: var(--radius);
    fill: none;
    stroke-width: var(--track-width);
  }

  .track {
    stroke: var(--track-color);
  }

  .indicator {
    stroke: var(--indicator-color);
    stroke-linecap: round;
    stroke-dasharray: calc(0.597 * var(--circumference)), calc(0.796 * var(--circumference));
    stroke-dashoffset: calc(-0.04 * var(--circumference));
    animation: dash 1.5s ease-in-out infinite;
  }

  @keyframes spin {
    0% {
      transform: rotate(0deg);
    }
    100% {
      transform: rotate(360deg);
    }
  }

  @keyframes dash {
    0% {
      stroke-dasharray: calc(0.008 * var(--circumference)), calc(1.194 * var(--circumference));
      stroke-dashoffset: 0;
    }
    50% {
      stroke-dasharray: calc(0.716 * var(--circumference)), calc(1.194 * var(--circumference));
      stroke-dashoffset: calc(-0.278 * var(--circumference));
    }
    100% {
      stroke-dasharray: calc(0.716 * var(--circumference)), calc(1.194 * var(--circumference));
      stroke-dashoffset: calc(-0.987 * var(--circumference));
    }
  }
`;var re=class extends G{constructor(){super(...arguments),this.localize=new Ht(this)}render(){return z`
      <svg
        part="base spinner"
        role="progressbar"
        aria-label=${this.localize.term("loading")}
        fill="none"
        xmlns="http://www.w3.org/2000/svg"
      >
        <circle class="track" />
        <circle class="indicator" />
      </svg>
    `}};re.css=Fr;re=d([Lt("wa-spinner")],re);var Ir=class extends Event{constructor(){super("wa-error",{bubbles:!0,cancelable:!1,composed:!0})}};var Rr=class extends Event{constructor(){super("wa-load",{bubbles:!0,cancelable:!1,composed:!0})}};var Nr=O`
  :host {
    --primary-color: currentColor;
    --primary-opacity: 1;
    --secondary-color: currentColor;
    --secondary-opacity: 0.4;
    --rotate-angle: 0deg;

    box-sizing: content-box;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    vertical-align: -0.125em;
  }

  /* #region Canvas — the box the icon is centered within (mirrors Font Awesome's icon canvas). Orthogonal to font-size. */

  /* Fixed width (default): 1.25em × 1em (20 × 16px) */
  :host(:not([canvas])),
  :host([canvas='fixed']) {
    width: 1.25em;
    height: 1em;
    min-width: 1.25em; /* <-- this is what Safari respects for intrinsic */
    min-height: 1em;
  }

  /* Auto: hug the icon's width. \`auto-width\` is the deprecated alias for canvas="auto". */
  :host([canvas='auto']),
  :host([auto-width]:not([canvas])) {
    width: auto;
    height: 1em;
  }

  /* Square: 1.25em × 1.25em (20 × 20px) */
  :host([canvas='square']) {
    width: 1.25em;
    height: 1.25em;
    min-width: 1.25em;
    min-height: 1.25em;
  }

  /* Roomy: 1.5em × 1.5em (24 × 24px) */
  :host([canvas='roomy']) {
    width: 1.5em;
    height: 1.5em;
    min-width: 1.5em;
    min-height: 1.5em;
  }

  /* #endregion */

  svg {
    /* NOTE: Avoid setting fill here. A stylesheet rule beats SVG presentation attributes, breaking stroke-based
       libraries like Lucide (fill="none" stroke="currentColor") and attribute-based mutators (issue #1733). The default
       library applies fill="currentColor" in its mutator instead. */
    height: 1em;
    overflow: visible;
    width: auto;

    /* Duotone colors with path-specific opacity fallback */
    path[data-duotone-primary] {
      color: var(--primary-color);
      opacity: var(--path-opacity, var(--primary-opacity));
    }

    path[data-duotone-secondary] {
      color: var(--secondary-color);
      opacity: var(--path-opacity, var(--secondary-opacity));
    }
  }

  /* Rotation */
  :host([rotate]) {
    transform: rotate(var(--rotate-angle, 0deg));
  }

  /* Flipping */
  :host([flip='x']) {
    transform: scaleX(-1);
  }
  :host([flip='y']) {
    transform: scaleY(-1);
  }
  :host([flip='both']) {
    transform: scale(-1, -1);
  }

  /* Rotation and Flipping combined */
  :host([rotate][flip='x']) {
    transform: rotate(var(--rotate-angle, 0deg)) scaleX(-1);
  }
  :host([rotate][flip='y']) {
    transform: rotate(var(--rotate-angle, 0deg)) scaleY(-1);
  }
  :host([rotate][flip='both']) {
    transform: rotate(var(--rotate-angle, 0deg)) scale(-1, -1);
  }

  /* #region Animations — ported from Font Awesome 7.3 (--fa-* props mapped to wa-icon's --* names) */

  :host([animation='beat']) {
    animation-name: beat;
    animation-delay: var(--animation-delay, 0s);
    animation-direction: var(--animation-direction, normal);
    animation-duration: var(--animation-duration, 1s);
    animation-iteration-count: var(--animation-iteration-count, infinite);
    animation-timing-function: var(--animation-timing, ease-in-out);
  }

  :host([animation='bounce']) {
    animation-name: bounce;
    animation-delay: var(--animation-delay, 0s);
    animation-direction: var(--animation-direction, normal);
    animation-duration: var(--animation-duration, 1s);
    animation-iteration-count: var(--animation-iteration-count, infinite);
    animation-timing-function: var(--animation-timing, cubic-bezier(0.28, 0.84, 0.42, 1));
  }

  :host([animation='fade']) {
    animation-name: fade;
    animation-delay: var(--animation-delay, 0s);
    animation-direction: var(--animation-direction, normal);
    animation-duration: var(--animation-duration, 1s);
    animation-iteration-count: var(--animation-iteration-count, infinite);
    animation-timing-function: var(--animation-timing, ease-in-out);
  }

  :host([animation='beat-fade']) {
    animation-name: beat-fade;
    animation-delay: var(--animation-delay, 0s);
    animation-direction: var(--animation-direction, normal);
    animation-duration: var(--animation-duration, 1s);
    animation-iteration-count: var(--animation-iteration-count, infinite);
    animation-timing-function: var(--animation-timing, ease-in-out);
  }

  :host([animation='flip']) {
    animation-name: flip;
    animation-delay: var(--animation-delay, 0s);
    animation-direction: var(--animation-direction, normal);
    animation-duration: var(--animation-duration, 1.5s);
    animation-iteration-count: var(--animation-iteration-count, infinite);
    animation-timing-function: var(--animation-timing, ease-in-out);
  }

  :host([animation='flip-360']) {
    animation-name: flip-360;
    animation-delay: var(--animation-delay, 0s);
    animation-direction: var(--animation-direction, normal);
    animation-duration: var(--animation-duration, 1s);
    animation-iteration-count: var(--animation-iteration-count, infinite);
    animation-timing-function: var(--animation-timing, ease-in-out);
  }

  :host([animation='shake']) {
    animation-name: shake;
    animation-delay: var(--animation-delay, 0s);
    animation-direction: var(--animation-direction, normal);
    animation-duration: var(--animation-duration, 0.75s);
    animation-iteration-count: var(--animation-iteration-count, infinite);
    animation-timing-function: var(--animation-timing, ease-in-out);
  }

  :host([animation='spin']) {
    animation-name: spin;
    animation-delay: var(--animation-delay, 0s);
    animation-direction: var(--animation-direction, normal);
    animation-duration: var(--animation-duration, 2s);
    animation-iteration-count: var(--animation-iteration-count, infinite);
    animation-timing-function: var(--animation-timing, linear);
  }

  :host([animation='spin-pulse']) {
    animation-name: spin;
    animation-delay: var(--animation-delay, 0s);
    animation-direction: var(--animation-direction, normal);
    animation-duration: var(--animation-duration, 1s);
    animation-iteration-count: var(--animation-iteration-count, infinite);
    animation-timing-function: var(--animation-timing, steps(8));
  }

  /* spin-reverse is FA's reverse modifier expressed as a standalone value; reverse any spin via --animation-direction: reverse */
  :host([animation='spin-reverse']) {
    animation-name: spin;
    animation-delay: var(--animation-delay, 0s);
    animation-direction: var(--animation-direction, reverse);
    animation-duration: var(--animation-duration, 2s);
    animation-iteration-count: var(--animation-iteration-count, infinite);
    animation-timing-function: var(--animation-timing, linear);
  }

  :host([animation='spin-snap']) {
    animation-name: spin-snap;
    animation-delay: var(--animation-delay, 0s);
    animation-direction: var(--animation-direction, normal);
    animation-duration: var(--animation-duration, 3s);
    animation-iteration-count: var(--animation-iteration-count, infinite);
    animation-timing-function: var(--animation-timing, linear);
  }

  :host([animation='spin-snap-4']) {
    animation-name: spin-snap-4;
    animation-delay: var(--animation-delay, 0s);
    animation-direction: var(--animation-direction, normal);
    animation-duration: var(--animation-duration, 2.4s);
    animation-iteration-count: var(--animation-iteration-count, infinite);
    animation-timing-function: var(--animation-timing, linear);
  }

  :host([animation='spin-snap-8']) {
    animation-name: spin-snap-8;
    animation-delay: var(--animation-delay, 0s);
    animation-direction: var(--animation-direction, normal);
    animation-duration: var(--animation-duration, 4s);
    animation-iteration-count: var(--animation-iteration-count, infinite);
    animation-timing-function: var(--animation-timing, linear);
  }

  :host([animation='buzz']) {
    animation-name: buzz;
    animation-delay: var(--animation-delay, 0s);
    animation-direction: var(--animation-direction, normal);
    animation-duration: var(--animation-duration, 0.6s);
    animation-iteration-count: var(--animation-iteration-count, infinite);
    animation-timing-function: var(--animation-timing, linear);
  }

  :host([animation='wag']) {
    animation-name: wag;
    animation-delay: var(--animation-delay, 0s);
    animation-direction: var(--animation-direction, normal);
    animation-duration: var(--animation-duration, 0.9s);
    animation-iteration-count: var(--animation-iteration-count, infinite);
    animation-timing-function: var(--animation-timing, ease-out);
    transform-origin: bottom center;
  }

  :host([animation='float']) {
    animation-name: float;
    animation-delay: var(--animation-delay, 0s);
    animation-direction: var(--animation-direction, normal);
    animation-duration: var(--animation-duration, 3s);
    animation-iteration-count: var(--animation-iteration-count, infinite);
    animation-timing-function: var(--animation-timing, ease-in-out);
    will-change: transform;
  }

  :host([animation='swing']) {
    animation-name: swing;
    animation-delay: var(--animation-delay, 0s);
    animation-direction: var(--animation-direction, normal);
    animation-duration: var(--animation-duration, 1.2s);
    animation-iteration-count: var(--animation-iteration-count, infinite);
    animation-timing-function: var(--animation-timing, ease-out);
    transform-origin: top center;
  }

  :host([animation='jello']) {
    animation-name: jello;
    animation-delay: var(--animation-delay, 0s);
    animation-direction: var(--animation-direction, normal);
    animation-duration: var(--animation-duration, 0.9s);
    animation-iteration-count: var(--animation-iteration-count, infinite);
    animation-timing-function: var(--animation-timing, ease-out);
  }

  @media (prefers-reduced-motion: reduce) {
    :host([animation='beat']),
    :host([animation='bounce']),
    :host([animation='fade']),
    :host([animation='beat-fade']),
    :host([animation='flip']),
    :host([animation='flip-360']),
    :host([animation='shake']),
    :host([animation='spin']),
    :host([animation='spin-pulse']),
    :host([animation='spin-reverse']),
    :host([animation='spin-snap']),
    :host([animation='spin-snap-4']),
    :host([animation='spin-snap-8']),
    :host([animation='buzz']),
    :host([animation='wag']),
    :host([animation='float']),
    :host([animation='swing']),
    :host([animation='jello']) {
      animation: none !important;
      transition: none !important;
    }
  }

  /* #endregion */

  /* #region Keyframes — ported verbatim from Font Awesome 7.3 */

  @keyframes beat {
    0% {
      transform: scale(1);
    }
    25% {
      transform: scale(calc(1.25 * var(--beat-scale, 1.25)));
    }
    45% {
      transform: scale(calc(1.22 * var(--beat-scale, 1.22)));
    }
    65% {
      transform: scale(calc(1.25 * var(--beat-scale, 1.25)));
    }
    90% {
      transform: scale(1);
    }
  }

  @keyframes bounce {
    0% {
      transform: scale(1, 1) translateY(0);
      /* No fallback by design (ported from FA 7.3): the first segment uses the user's --animation-timing or the CSS
         initial ease, while the explicit cubic-beziers on later stops drive the bounce physics. */
      animation-timing-function: var(--animation-timing);
    }
    14% {
      transform: scale(var(--bounce-start-scale-x, 1.06), var(--bounce-start-scale-y, 0.94))
        translateY(var(--bounce-anticipation, 3px));
      animation-timing-function: cubic-bezier(0.33, 0, 0.66, 0.33);
    }
    32% {
      transform: scale(var(--bounce-jump-scale-x, 0.94), var(--bounce-jump-scale-y, 1.12))
        translateY(calc(-1 * var(--bounce-height, 0.5em)));
      animation-timing-function: cubic-bezier(0.33, 0.66, 0.66, 1);
    }
    52% {
      transform: scale(1, 1) translateY(calc(-1 * var(--bounce-height, 0.5em) * 1.1));
      animation-timing-function: cubic-bezier(0.5, 0, 1, 0.5);
    }
    70% {
      transform: scale(var(--bounce-land-scale-x, 1.06), var(--bounce-land-scale-y, 0.92)) translateY(0);
      animation-timing-function: cubic-bezier(0.33, 0.33, 0.66, 1);
    }
    85% {
      transform: scale(0.98, 1.04) translateY(calc(-2px * var(--bounce-rebound, 1)));
      animation-timing-function: cubic-bezier(0.33, 0, 0.66, 1);
    }
    100% {
      transform: scale(1, 1) translateY(0);
    }
  }

  @keyframes fade {
    0% {
      opacity: 1;
      transform: scale(1);
      animation-timing-function: cubic-bezier(0.2, 0, 0.4, 1);
    }
    40% {
      opacity: var(--fade-opacity, 0.4);
      transform: scale(0.98);
      animation-timing-function: cubic-bezier(0.4, 0, 0.6, 1);
    }
    100% {
      opacity: 1;
      transform: scale(1);
    }
  }

  @keyframes beat-fade {
    0% {
      opacity: var(--beat-fade-opacity, 0.4);
      transform: scale(1);
      animation-timing-function: cubic-bezier(0.2, 0, 0.4, 1);
    }
    25% {
      opacity: calc(var(--beat-fade-opacity, 0.4) + 0.4);
      transform: scale(var(--beat-fade-scale, 1.28));
      animation-timing-function: cubic-bezier(0.4, 0, 0.6, 1);
    }
    45% {
      opacity: 1;
      transform: scale(var(--beat-fade-scale, 1.25));
      animation-timing-function: cubic-bezier(0.4, 0, 0.2, 1);
    }
    65% {
      opacity: calc(var(--beat-fade-opacity, 0.4) + 0.4);
      transform: scale(var(--beat-fade-scale, 1.28));
      animation-timing-function: cubic-bezier(0.4, 0, 0.6, 1);
    }
    100% {
      opacity: var(--beat-fade-opacity, 0.4);
      transform: scale(1);
    }
  }

  @keyframes flip {
    0% {
      transform: perspective(2em) scale(1) rotate3d(var(--flip-x, 0), var(--flip-y, 1), var(--flip-z, 0), 0deg);
      animation-timing-function: cubic-bezier(0.2, 0, 0.4, 1);
    }
    8% {
      transform: perspective(2em) scale(var(--flip-anticipation-scale, 0.95))
        rotate3d(var(--flip-x, 0), var(--flip-y, 1), var(--flip-z, 0), 0deg);
      animation-timing-function: cubic-bezier(0.33, 0, 0.66, 0.33);
    }
    35% {
      transform: perspective(2em) scale(1)
        rotate3d(var(--flip-x, 0), var(--flip-y, 1), var(--flip-z, 0), calc(var(--flip-angle, -360deg) * 0.6));
      animation-timing-function: linear;
    }
    65% {
      transform: perspective(2em) scale(1)
        rotate3d(var(--flip-x, 0), var(--flip-y, 1), var(--flip-z, 0), calc(var(--flip-angle, -360deg) * 0.5));
      animation-timing-function: cubic-bezier(0.33, 0.66, 0.66, 1);
    }
    92% {
      transform: perspective(2em) scale(1)
        rotate3d(
          var(--flip-x, 0),
          var(--flip-y, 1),
          var(--flip-z, 0),
          calc(var(--flip-angle, -360deg) * var(--flip-overshoot, 1.04))
        );
      animation-timing-function: cubic-bezier(0.33, 0, 0.66, 1);
    }
    100% {
      transform: perspective(2em) scale(1)
        rotate3d(var(--flip-x, 0), var(--flip-y, 1), var(--flip-z, 0), var(--flip-angle, -360deg));
    }
  }

  @keyframes flip-360 {
    0% {
      transform: perspective(2em) scale(1) rotate3d(var(--flip-x, 0), var(--flip-y, 1), var(--flip-z, 0), 0deg);
      animation-timing-function: cubic-bezier(0.2, 0, 0.4, 1);
    }
    8% {
      transform: perspective(2em) scale(var(--flip-anticipation-scale, 0.95))
        rotate3d(var(--flip-x, 0), var(--flip-y, 1), var(--flip-z, 0), 0deg);
      animation-timing-function: cubic-bezier(0.33, 0, 0.66, 0.33);
    }
    50% {
      transform: perspective(2em) scale(1)
        rotate3d(var(--flip-x, 0), var(--flip-y, 1), var(--flip-z, 0), calc(var(--flip-angle, -360deg) * 0.6));
      animation-timing-function: cubic-bezier(0.33, 0.66, 0.66, 1);
    }
    80% {
      transform: perspective(2em) scale(1)
        rotate3d(
          var(--flip-x, 0),
          var(--flip-y, 1),
          var(--flip-z, 0),
          calc(var(--flip-angle, -360deg) * var(--flip-overshoot, 1.04))
        );
      animation-timing-function: cubic-bezier(0.33, 0, 0.66, 1);
    }
    100% {
      transform: perspective(2em) scale(1)
        rotate3d(var(--flip-x, 0), var(--flip-y, 1), var(--flip-z, 0), var(--flip-angle, -360deg));
    }
  }

  @keyframes shake {
    0% {
      transform: rotate(0deg);
      animation-timing-function: cubic-bezier(0.2, 0, 0.8, 1);
    }
    8% {
      transform: rotate(35deg) translateX(1px);
      animation-timing-function: cubic-bezier(0.3, 0, 0.7, 1);
    }
    20% {
      transform: rotate(-22deg) translateX(-1px);
      animation-timing-function: cubic-bezier(0.3, 0, 0.7, 1);
    }
    35% {
      transform: rotate(15deg) translateX(1px);
      animation-timing-function: cubic-bezier(0.3, 0, 0.7, 1);
    }
    50% {
      transform: rotate(-9deg);
      animation-timing-function: cubic-bezier(0.4, 0, 0.6, 1);
    }
    65% {
      transform: rotate(5deg);
      animation-timing-function: cubic-bezier(0.4, 0, 0.6, 1);
    }
    78% {
      transform: rotate(-3deg);
      animation-timing-function: cubic-bezier(0.4, 0, 0.6, 1);
    }
    90% {
      transform: rotate(1deg);
      animation-timing-function: cubic-bezier(0.4, 0, 0.2, 1);
    }
    100% {
      transform: rotate(0deg);
    }
  }

  @keyframes spin {
    0% {
      transform: rotate(0deg);
    }
    100% {
      transform: rotate(360deg);
    }
  }

  @keyframes spin-snap {
    0% {
      transform: rotate(0deg);
      animation-timing-function: cubic-bezier(0, 0, 0.2, 1);
    }
    12% {
      transform: rotate(60deg);
      animation-timing-function: cubic-bezier(0.8, 0, 1, 1);
    }
    16.67% {
      transform: rotate(60deg);
      animation-timing-function: cubic-bezier(0, 0, 0.2, 1);
    }
    28.67% {
      transform: rotate(120deg);
      animation-timing-function: cubic-bezier(0.8, 0, 1, 1);
    }
    33.33% {
      transform: rotate(120deg);
      animation-timing-function: cubic-bezier(0, 0, 0.2, 1);
    }
    45.33% {
      transform: rotate(180deg);
      animation-timing-function: cubic-bezier(0.8, 0, 1, 1);
    }
    50% {
      transform: rotate(180deg);
      animation-timing-function: cubic-bezier(0, 0, 0.2, 1);
    }
    62% {
      transform: rotate(240deg);
      animation-timing-function: cubic-bezier(0.8, 0, 1, 1);
    }
    66.67% {
      transform: rotate(240deg);
      animation-timing-function: cubic-bezier(0, 0, 0.2, 1);
    }
    78.67% {
      transform: rotate(300deg);
      animation-timing-function: cubic-bezier(0.8, 0, 1, 1);
    }
    83.33% {
      transform: rotate(300deg);
      animation-timing-function: cubic-bezier(0, 0, 0.2, 1);
    }
    95.33% {
      transform: rotate(360deg);
      animation-timing-function: cubic-bezier(0.8, 0, 1, 1);
    }
    100% {
      transform: rotate(360deg);
    }
  }

  @keyframes spin-snap-4 {
    0% {
      transform: rotate(0deg);
      animation-timing-function: cubic-bezier(0, 0, 0.2, 1);
    }
    15% {
      transform: rotate(90deg);
      animation-timing-function: cubic-bezier(0.8, 0, 1, 1);
    }
    25% {
      transform: rotate(90deg);
      animation-timing-function: cubic-bezier(0, 0, 0.2, 1);
    }
    40% {
      transform: rotate(180deg);
      animation-timing-function: cubic-bezier(0.8, 0, 1, 1);
    }
    50% {
      transform: rotate(180deg);
      animation-timing-function: cubic-bezier(0, 0, 0.2, 1);
    }
    65% {
      transform: rotate(270deg);
      animation-timing-function: cubic-bezier(0.8, 0, 1, 1);
    }
    75% {
      transform: rotate(270deg);
      animation-timing-function: cubic-bezier(0, 0, 0.2, 1);
    }
    90% {
      transform: rotate(360deg);
      animation-timing-function: cubic-bezier(0.8, 0, 1, 1);
    }
    100% {
      transform: rotate(360deg);
    }
  }

  @keyframes spin-snap-8 {
    0% {
      transform: rotate(0deg);
      animation-timing-function: cubic-bezier(0, 0, 0.2, 1);
    }
    9% {
      transform: rotate(45deg);
      animation-timing-function: cubic-bezier(0.8, 0, 1, 1);
    }
    12.5% {
      transform: rotate(45deg);
      animation-timing-function: cubic-bezier(0, 0, 0.2, 1);
    }
    21.5% {
      transform: rotate(90deg);
      animation-timing-function: cubic-bezier(0.8, 0, 1, 1);
    }
    25% {
      transform: rotate(90deg);
      animation-timing-function: cubic-bezier(0, 0, 0.2, 1);
    }
    34% {
      transform: rotate(135deg);
      animation-timing-function: cubic-bezier(0.8, 0, 1, 1);
    }
    37.5% {
      transform: rotate(135deg);
      animation-timing-function: cubic-bezier(0, 0, 0.2, 1);
    }
    46.5% {
      transform: rotate(180deg);
      animation-timing-function: cubic-bezier(0.8, 0, 1, 1);
    }
    50% {
      transform: rotate(180deg);
      animation-timing-function: cubic-bezier(0, 0, 0.2, 1);
    }
    59% {
      transform: rotate(225deg);
      animation-timing-function: cubic-bezier(0.8, 0, 1, 1);
    }
    62.5% {
      transform: rotate(225deg);
      animation-timing-function: cubic-bezier(0, 0, 0.2, 1);
    }
    71.5% {
      transform: rotate(270deg);
      animation-timing-function: cubic-bezier(0.8, 0, 1, 1);
    }
    75% {
      transform: rotate(270deg);
      animation-timing-function: cubic-bezier(0, 0, 0.2, 1);
    }
    84% {
      transform: rotate(315deg);
      animation-timing-function: cubic-bezier(0.8, 0, 1, 1);
    }
    87.5% {
      transform: rotate(315deg);
      animation-timing-function: cubic-bezier(0, 0, 0.2, 1);
    }
    96.5% {
      transform: rotate(360deg);
      animation-timing-function: cubic-bezier(0.8, 0, 1, 1);
    }
    100% {
      transform: rotate(360deg);
    }
  }

  @keyframes buzz {
    0% {
      transform: translateX(0) rotate(0deg);
      animation-timing-function: cubic-bezier(0.1, 0, 0.9, 1);
    }
    5% {
      transform: translateX(var(--buzz-distance, 4px)) rotate(0.5deg);
    }
    10% {
      transform: translateX(calc(-1 * var(--buzz-distance, 4px))) rotate(-0.5deg);
    }
    15% {
      transform: translateX(var(--buzz-distance, 4px)) rotate(0.3deg);
    }
    20% {
      transform: translateX(calc(-1 * var(--buzz-distance, 4px))) rotate(-0.3deg);
    }
    25% {
      transform: translateX(calc(var(--buzz-distance, 4px) * 0.7)) rotate(0.2deg);
    }
    30% {
      transform: translateX(calc(-1 * var(--buzz-distance, 4px) * 0.7)) rotate(-0.2deg);
    }
    35% {
      transform: translateX(calc(var(--buzz-distance, 4px) * 0.4)) rotate(0.1deg);
    }
    40% {
      transform: translateX(0) rotate(0deg);
    }
    100% {
      transform: translateX(0) rotate(0deg);
    }
  }

  @keyframes wag {
    0% {
      transform: rotate(0deg);
      animation-timing-function: cubic-bezier(0.2, 0, 0.6, 1);
    }
    12% {
      transform: rotate(var(--wag-angle, 12deg));
      animation-timing-function: cubic-bezier(0.4, 0, 0.2, 1);
    }
    24% {
      transform: rotate(2deg);
      animation-timing-function: cubic-bezier(0.2, 0, 0.6, 1);
    }
    36% {
      transform: rotate(calc(var(--wag-angle, 12deg) * 0.85));
      animation-timing-function: cubic-bezier(0.4, 0, 0.2, 1);
    }
    48% {
      transform: rotate(1deg);
      animation-timing-function: cubic-bezier(0.2, 0, 0.6, 1);
    }
    58% {
      transform: rotate(calc(var(--wag-angle, 12deg) * 0.6));
      animation-timing-function: cubic-bezier(0.4, 0, 0.2, 1);
    }
    68% {
      transform: rotate(0deg);
    }
    100% {
      transform: rotate(0deg);
    }
  }

  @keyframes float {
    0% {
      transform: translateY(0) translateX(0) rotate(0deg)
        scale(var(--float-squash-x, 1.02), var(--float-squash-y, 0.98));
      animation-timing-function: cubic-bezier(0.33, 0, 0.66, 0.33);
    }
    15% {
      transform: translateY(calc(-0.4 * var(--float-height, 6px))) translateX(var(--float-drift, 1px))
        rotate(var(--float-tilt, 1deg)) scale(1, 1);
      animation-timing-function: cubic-bezier(0.33, 0.66, 0.66, 1);
    }
    35% {
      transform: translateY(calc(-1 * var(--float-height, 6px))) translateX(0) rotate(0deg)
        scale(var(--float-stretch-x, 0.98), var(--float-stretch-y, 1.03));
      animation-timing-function: cubic-bezier(0.5, 0, 0.5, 0);
    }
    50% {
      transform: translateY(calc(-0.92 * var(--float-height, 6px))) translateX(calc(-0.5 * var(--float-drift, 1px)))
        rotate(calc(-0.5 * var(--float-tilt, 1deg))) scale(0.995, 1.01);
      animation-timing-function: cubic-bezier(0.33, 0, 0.66, 0.33);
    }
    70% {
      transform: translateY(calc(-0.3 * var(--float-height, 6px))) translateX(calc(-1 * var(--float-drift, 1px)))
        rotate(calc(-1 * var(--float-tilt, 1deg))) scale(1, 1);
      animation-timing-function: cubic-bezier(0.33, 0.66, 0.66, 1);
    }
    90% {
      transform: translateY(calc(0.05 * var(--float-height, 6px))) translateX(0) rotate(0deg)
        scale(var(--float-squash-x, 1.02), var(--float-squash-y, 0.98));
      animation-timing-function: cubic-bezier(0.33, 0, 0.66, 1);
    }
    100% {
      transform: translateY(0) translateX(0) rotate(0deg)
        scale(var(--float-squash-x, 1.02), var(--float-squash-y, 0.98));
    }
  }

  @keyframes swing {
    0% {
      transform: rotate(0deg);
      animation-timing-function: cubic-bezier(0.2, 0, 0.8, 1);
    }
    8% {
      transform: rotate(var(--swing-angle, 22deg));
      animation-timing-function: cubic-bezier(0.3, 0, 0.7, 1);
    }
    18% {
      transform: rotate(calc(-1 * var(--swing-angle, 22deg) * 0.85));
      animation-timing-function: cubic-bezier(0.3, 0, 0.7, 1);
    }
    28% {
      transform: rotate(calc(var(--swing-angle, 22deg) * 0.65));
      animation-timing-function: cubic-bezier(0.35, 0, 0.65, 1);
    }
    38% {
      transform: rotate(calc(-1 * var(--swing-angle, 22deg) * 0.45));
      animation-timing-function: cubic-bezier(0.4, 0, 0.6, 1);
    }
    48% {
      transform: rotate(calc(var(--swing-angle, 22deg) * 0.25));
      animation-timing-function: cubic-bezier(0.4, 0, 0.6, 1);
    }
    56% {
      transform: rotate(calc(-1 * var(--swing-angle, 22deg) * 0.1));
      animation-timing-function: cubic-bezier(0.4, 0, 0.6, 1);
    }
    64% {
      transform: rotate(0deg);
    }
    100% {
      transform: rotate(0deg);
    }
  }

  @keyframes jello {
    0% {
      transform: scale(1, 1);
      animation-timing-function: cubic-bezier(0.2, 0, 0.8, 1);
    }
    12% {
      transform: scale(var(--jello-scale-x, 1.15), calc(2 - var(--jello-scale-x, 1.15)));
      animation-timing-function: cubic-bezier(0.3, 0, 0.7, 1);
    }
    24% {
      transform: scale(calc(2 - var(--jello-scale-y, 1.12)), var(--jello-scale-y, 1.12));
      animation-timing-function: cubic-bezier(0.3, 0, 0.7, 1);
    }
    36% {
      transform: scale(
        calc(1 + (var(--jello-scale-x, 1.15) - 1) * 0.5),
        calc(2 - (1 + (var(--jello-scale-x, 1.15) - 1) * 0.5))
      );
      animation-timing-function: cubic-bezier(0.4, 0, 0.6, 1);
    }
    48% {
      transform: scale(
        calc(2 - (1 + (var(--jello-scale-y, 1.12) - 1) * 0.3)),
        calc(1 + (var(--jello-scale-y, 1.12) - 1) * 0.3)
      );
      animation-timing-function: cubic-bezier(0.4, 0, 0.6, 1);
    }
    58% {
      transform: scale(1.02, 0.98);
      animation-timing-function: cubic-bezier(0.4, 0, 0.2, 1);
    }
    68% {
      transform: scale(1, 1);
    }
    100% {
      transform: scale(1, 1);
    }
  }

  /* #endregion */
`;var Or="",ae="";function Dr(){return Or.replace(/\/$/,"")}function Br(t){ae=t}function Pr(){if(!ae){const t=document.querySelector("[data-fa-kit-code]");t&&Br(t.getAttribute("data-fa-kit-code")||"")}return ae}var gn="7.3.0";function Ur(t,e,n){let i="solid";return e==="chisel"&&(i="chisel-regular"),e==="etch"&&(i="etch-solid"),e==="graphite"&&(i="graphite-thin"),e==="jelly"&&(i="jelly-regular",n==="duo-regular"&&(i="jelly-duo-regular"),n==="fill-regular"&&(i="jelly-fill-regular")),e==="jelly-duo"&&(i="jelly-duo-regular"),e==="jelly-fill"&&(i="jelly-fill-regular"),e==="notdog"&&(n==="solid"&&(i="notdog-solid"),n==="duo-solid"&&(i="notdog-duo-solid")),e==="notdog-duo"&&(i="notdog-duo-solid"),e==="slab"&&((n==="solid"||n==="regular")&&(i="slab-regular"),n==="press-regular"&&(i="slab-press-regular")),e==="slab-press"&&(i="slab-press-regular"),e==="slab-duo"&&(i="slab-duo-regular"),e==="slab-press-duo"&&(i="slab-press-duo-regular"),e==="thumbprint"&&(i="thumbprint-light"),e==="utility"&&(i="utility-semibold"),e==="utility-duo"&&(i="utility-duo-semibold"),e==="utility-fill"&&(i="utility-fill-semibold"),e==="whiteboard"&&(i="whiteboard-semibold"),e==="mosaic"&&(i="mosaic-solid"),e==="pixel"&&(i="pixel-regular"),e==="vellum"&&(i="vellum-solid"),e==="classic"&&(n==="thin"&&(i="thin"),n==="light"&&(i="light"),n==="regular"&&(i="regular"),n==="solid"&&(i="solid")),e==="duotone"&&(n==="thin"&&(i="duotone-thin"),n==="light"&&(i="duotone-light"),n==="regular"&&(i="duotone-regular"),n==="solid"&&(i="duotone")),e==="sharp"&&(n==="thin"&&(i="sharp-thin"),n==="light"&&(i="sharp-light"),n==="regular"&&(i="sharp-regular"),n==="solid"&&(i="sharp-solid")),e==="sharp-duotone"&&(n==="thin"&&(i="sharp-duotone-thin"),n==="light"&&(i="sharp-duotone-light"),n==="regular"&&(i="sharp-duotone-regular"),n==="solid"&&(i="sharp-duotone-solid")),e==="brands"&&(i="brands"),i}function qr(t,e,n){const i=Ur(t,e,n),o=Dr();if(o)return`${o}/${i}/${t}.svg`;const r=Pr();return r.length>0?`https://ka-p.fontawesome.com/releases/v${gn}/svgs/${i}/${t}.svg?token=${encodeURIComponent(r)}`:`https://ka-f.fontawesome.com/releases/v${gn}/svgs/${i}/${t}.svg`}var Vr={name:"default",resolver:(t,e="classic",n="solid")=>qr(t,e,n),mutator:(t,e)=>{if(t.hasAttribute("fill")||t.setAttribute("fill","currentColor"),e?.family&&!t.hasAttribute("data-duotone-initialized")){const{family:n,variant:i}=e;if(n==="duotone"||n==="sharp-duotone"||n==="notdog-duo"||n==="notdog"&&i==="duo-solid"||n==="jelly-duo"||n==="jelly"&&i==="duo-regular"||n==="utility-duo"||n==="slab-duo"||n==="slab-press-duo"||n==="thumbprint"){const o=[...t.querySelectorAll("path")],r=o.find(s=>!s.hasAttribute("opacity")),a=o.find(s=>s.hasAttribute("opacity"));if(!r||!a)return;if(r.setAttribute("data-duotone-primary",""),a.setAttribute("data-duotone-secondary",""),e.swapOpacity&&r&&a){const s=a.getAttribute("opacity")||"0.4";r.style.setProperty("--path-opacity",s),a.style.setProperty("--path-opacity","1")}t.setAttribute("data-duotone-initialized","")}}}},jr=Vr;function Wr(t){return`data:image/svg+xml,${encodeURIComponent(t)}`}var Gt={solid:{backward:'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 640 640"><!--!Font Awesome Free v7.2.0 by @fontawesome - https://fontawesome.com License - https://fontawesome.com/license/free Copyright 2026 Fonticons, Inc.--><path fill="currentColor" d="M236.3 107.1C247.9 96 265 92.9 279.7 99.2C294.4 105.5 304 120 304 136L304 272.3L476.3 107.2C487.9 96 505 92.9 519.7 99.2C534.4 105.5 544 120 544 136L544 504C544 520 534.4 534.5 519.7 540.8C505 547.1 487.9 544 476.3 532.9L304 367.7L304 504C304 520 294.4 534.5 279.7 540.8C265 547.1 247.9 544 236.3 532.9L44.3 348.9C36.5 341.3 32 330.9 32 320C32 309.1 36.5 298.7 44.3 291.1L236.3 107.1z"/></svg>',"backward-step":'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 640 640"><!--!Font Awesome Free v7.2.0 by @fontawesome - https://fontawesome.com License - https://fontawesome.com/license/free Copyright 2026 Fonticons, Inc.--><path fill="currentColor" d="M491 100.8C478.1 93.8 462.3 94.5 450 102.6L192 272.1L192 128C192 110.3 177.7 96 160 96C142.3 96 128 110.3 128 128L128 512C128 529.7 142.3 544 160 544C177.7 544 192 529.7 192 512L192 367.9L450 537.5C462.3 545.6 478 546.3 491 539.3C504 532.3 512 518.8 512 504.1L512 136.1C512 121.4 503.9 107.9 491 100.9z"/></svg>',"angles-left":'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 512 512"><!--! Font Awesome Free 7.0.0 by @fontawesome - https://fontawesome.com License - https://fontawesome.com/license/free Copyright 2025 Fonticons, Inc. --><path fill="currentColor" d="M77.3 256 214.7 118.6c12.5-12.5 12.5-32.8 0-45.3s-32.8-12.5-45.3 0l-160 160c-12.5 12.5-12.5 32.8 0 45.3l160 160c12.5 12.5 32.8 12.5 45.3 0s12.5-32.8 0-45.3L77.3 256zm192 0L406.7 118.6c12.5-12.5 12.5-32.8 0-45.3s-32.8-12.5-45.3 0l-160 160c-12.5 12.5-12.5 32.8 0 45.3l160 160c12.5 12.5 32.8 12.5 45.3 0s12.5-32.8 0-45.3L269.3 256z"/></svg>',"angles-right":'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 512 512"><!--! Font Awesome Free 7.0.0 by @fontawesome - https://fontawesome.com License - https://fontawesome.com/license/free Copyright 2025 Fonticons, Inc. --><path fill="currentColor" d="M434.7 256 297.3 118.6c-12.5-12.5-12.5-32.8 0-45.3s32.8-12.5 45.3 0l160 160c12.5 12.5 12.5 32.8 0 45.3l-160 160c-12.5 12.5-32.8 12.5-45.3 0s-12.5-32.8 0-45.3L434.7 256zm-192 0L105.3 118.6c-12.5-12.5-12.5-32.8 0-45.3s32.8-12.5 45.3 0l160 160c12.5 12.5 12.5 32.8 0 45.3l-160 160c-12.5 12.5-32.8 12.5-45.3 0s-12.5-32.8 0-45.3L242.7 256z"/></svg>',check:'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 448 512"><!--! Font Awesome Free 7.0.0 by @fontawesome - https://fontawesome.com License - https://fontawesome.com/license/free Copyright 2025 Fonticons, Inc. --><path fill="currentColor" d="M434.8 70.1c14.3 10.4 17.5 30.4 7.1 44.7l-256 352c-5.5 7.6-14 12.3-23.4 13.1s-18.5-2.7-25.1-9.3l-128-128c-12.5-12.5-12.5-32.8 0-45.3s32.8-12.5 45.3 0l101.5 101.5 234-321.7c10.4-14.3 30.4-17.5 44.7-7.1z"/></svg>',"chevron-down":'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 448 512"><!--! Font Awesome Free 7.0.0 by @fontawesome - https://fontawesome.com License - https://fontawesome.com/license/free Copyright 2025 Fonticons, Inc. --><path fill="currentColor" d="M201.4 406.6c12.5 12.5 32.8 12.5 45.3 0l192-192c12.5-12.5 12.5-32.8 0-45.3s-32.8-12.5-45.3 0L224 338.7 54.6 169.4c-12.5-12.5-32.8-12.5-45.3 0s-12.5 32.8 0 45.3l192 192z"/></svg>',"chevron-left":'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 320 512"><!--! Font Awesome Free 7.0.0 by @fontawesome - https://fontawesome.com License - https://fontawesome.com/license/free Copyright 2025 Fonticons, Inc. --><path fill="currentColor" d="M9.4 233.4c-12.5 12.5-12.5 32.8 0 45.3l192 192c12.5 12.5 32.8 12.5 45.3 0s12.5-32.8 0-45.3L77.3 256 246.6 86.6c12.5-12.5 12.5-32.8 0-45.3s-32.8-12.5-45.3 0l-192 192z"/></svg>',"chevron-right":'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 320 512"><!--! Font Awesome Free 7.0.0 by @fontawesome - https://fontawesome.com License - https://fontawesome.com/license/free Copyright 2025 Fonticons, Inc. --><path fill="currentColor" d="M311.1 233.4c12.5 12.5 12.5 32.8 0 45.3l-192 192c-12.5 12.5-32.8 12.5-45.3 0s-12.5-32.8 0-45.3L243.2 256 73.9 86.6c-12.5-12.5-12.5-32.8 0-45.3s32.8-12.5 45.3 0l192 192z"/></svg>',circle:'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 512 512"><!--! Font Awesome Free 7.0.0 by @fontawesome - https://fontawesome.com License - https://fontawesome.com/license/free Copyright 2025 Fonticons, Inc. --><path fill="currentColor" d="M0 256a256 256 0 1 1 512 0 256 256 0 1 1 -512 0z"/></svg>',"closed-captioning":'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 640 640"><!--!Font Awesome Free v7.2.0 by @fontawesome - https://fontawesome.com License - https://fontawesome.com/license/free Copyright 2026 Fonticons, Inc.--><path fill="currentColor" d="M64 192C64 156.7 92.7 128 128 128L512 128C547.3 128 576 156.7 576 192L576 448C576 483.3 547.3 512 512 512L128 512C92.7 512 64 483.3 64 448L64 192zM216 272L248 272C252.4 272 256 275.6 256 280C256 293.3 266.7 304 280 304C293.3 304 304 293.3 304 280C304 249.1 278.9 224 248 224L216 224C185.1 224 160 249.1 160 280L160 360C160 390.9 185.1 416 216 416L248 416C278.9 416 304 390.9 304 360C304 346.7 293.3 336 280 336C266.7 336 256 346.7 256 360C256 364.4 252.4 368 248 368L216 368C211.6 368 208 364.4 208 360L208 280C208 275.6 211.6 272 216 272zM384 280C384 275.6 387.6 272 392 272L424 272C428.4 272 432 275.6 432 280C432 293.3 442.7 304 456 304C469.3 304 480 293.3 480 280C480 249.1 454.9 224 424 224L392 224C361.1 224 336 249.1 336 280L336 360C336 390.9 361.1 416 392 416L424 416C454.9 416 480 390.9 480 360C480 346.7 469.3 336 456 336C442.7 336 432 346.7 432 360C432 364.4 428.4 368 424 368L392 368C387.6 368 384 364.4 384 360L384 280z"/></svg>',"closed-captioning-slash":'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 640 640"><!--!Font Awesome Free v7.2.0 by @fontawesome - https://fontawesome.com License - https://fontawesome.com/license/free Copyright 2026 Fonticons, Inc.--><path fill="currentColor" d="M39 39.1C48.4 29.7 63.6 29.7 72.9 39.1L161.8 128L512 128C547.3 128 576 156.7 576 192L576 448C576 473.5 561.1 495.4 539.6 505.8L601 567.1C610.4 576.5 610.4 591.7 601 601C591.6 610.3 576.4 610.4 567.1 601L39 73.1C29.7 63.7 29.7 48.5 39 39.1zM384 350.1L384 279.9C384 275.5 387.6 271.9 392 271.9L424 271.9C428.4 271.9 432 275.5 432 279.9C432 293.2 442.7 303.9 456 303.9C469.3 303.9 480 293.2 480 279.9C480 249 454.9 223.9 424 223.9L392 223.9C361.1 223.9 336 249 336 279.9L336 302.1L384 350.1zM445.5 411.6C465.7 403.2 480 383.2 480 359.9C480 346.6 469.3 335.9 456 335.9C442.7 335.9 432 346.6 432 359.9C432 364.3 428.4 367.9 424 367.9L401.8 367.9L445.5 411.6zM162.3 264.1C160.8 269.1 160 274.5 160 280L160 360C160 390.9 185.1 416 216 416L248 416C266.1 416 282.1 407.5 292.4 394.2L410.2 512L128 512C92.7 512 64 483.3 64 448L64 192C64 184.2 65.4 176.7 68 169.8L162.3 264.1zM256.1 357.9C256 358.6 256 359.3 256 360C256 364.4 252.4 368 248 368L216 368C211.6 368 208 364.4 208 360L208 309.8L256.1 357.9z"/></svg>',compress:'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 448 512"><!--!Font Awesome Free v7.2.0 by @fontawesome - https://fontawesome.com License - https://fontawesome.com/license/free Copyright 2026 Fonticons, Inc.--><path fill="currentColor" d="M160 64c0-17.7-14.3-32-32-32S96 46.3 96 64l0 64-64 0c-17.7 0-32 14.3-32 32s14.3 32 32 32l96 0c17.7 0 32-14.3 32-32l0-96zM32 320c-17.7 0-32 14.3-32 32s14.3 32 32 32l64 0 0 64c0 17.7 14.3 32 32 32s32-14.3 32-32l0-96c0-17.7-14.3-32-32-32l-96 0zM352 64c0-17.7-14.3-32-32-32s-32 14.3-32 32l0 96c0 17.7 14.3 32 32 32l96 0c17.7 0 32-14.3 32-32s-14.3-32-32-32l-64 0 0-64zM320 320c-17.7 0-32 14.3-32 32l0 96c0 17.7 14.3 32 32 32s32-14.3 32-32l0-64 64 0c17.7 0 32-14.3 32-32s-14.3-32-32-32l-96 0z"/></svg>',ellipsis:'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 640 640"><!--!Font Awesome Free v7.3.0 by @fontawesome - https://fontawesome.com License - https://fontawesome.com/license/free Copyright 2026 Fonticons, Inc.--><path d="M96 320C96 289.1 121.1 264 152 264C182.9 264 208 289.1 208 320C208 350.9 182.9 376 152 376C121.1 376 96 350.9 96 320zM264 320C264 289.1 289.1 264 320 264C350.9 264 376 289.1 376 320C376 350.9 350.9 376 320 376C289.1 376 264 350.9 264 320zM488 264C518.9 264 544 289.1 544 320C544 350.9 518.9 376 488 376C457.1 376 432 350.9 432 320C432 289.1 457.1 264 488 264z"/></svg>',"ellipsis-vertical":'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 640 640"><!--!Font Awesome Free v7.2.0 by @fontawesome - https://fontawesome.com License - https://fontawesome.com/license/free Copyright 2026 Fonticons, Inc.--><path fill="currentColor" d="M320 208C289.1 208 264 182.9 264 152C264 121.1 289.1 96 320 96C350.9 96 376 121.1 376 152C376 182.9 350.9 208 320 208zM320 432C350.9 432 376 457.1 376 488C376 518.9 350.9 544 320 544C289.1 544 264 518.9 264 488C264 457.1 289.1 432 320 432zM376 320C376 350.9 350.9 376 320 376C289.1 376 264 350.9 264 320C264 289.1 289.1 264 320 264C350.9 264 376 289.1 376 320z"/></svg>',expand:'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 640 640"><!--!Font Awesome Free v7.2.0 by @fontawesome - https://fontawesome.com License - https://fontawesome.com/license/free Copyright 2026 Fonticons, Inc.--><path fill="currentColor" d="M128 96C110.3 96 96 110.3 96 128L96 224C96 241.7 110.3 256 128 256C145.7 256 160 241.7 160 224L160 160L224 160C241.7 160 256 145.7 256 128C256 110.3 241.7 96 224 96L128 96zM160 416C160 398.3 145.7 384 128 384C110.3 384 96 398.3 96 416L96 512C96 529.7 110.3 544 128 544L224 544C241.7 544 256 529.7 256 512C256 494.3 241.7 480 224 480L160 480L160 416zM416 96C398.3 96 384 110.3 384 128C384 145.7 398.3 160 416 160L480 160L480 224C480 241.7 494.3 256 512 256C529.7 256 544 241.7 544 224L544 128C544 110.3 529.7 96 512 96L416 96zM544 416C544 398.3 529.7 384 512 384C494.3 384 480 398.3 480 416L480 480L416 480C398.3 480 384 494.3 384 512C384 529.7 398.3 544 416 544L512 544C529.7 544 544 529.7 544 512L544 416z"/></svg>',eyedropper:'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 512 512"><!--! Font Awesome Free 7.0.0 by @fontawesome - https://fontawesome.com License - https://fontawesome.com/license/free Copyright 2025 Fonticons, Inc. --><path fill="currentColor" d="M341.6 29.2l-101.6 101.6-9.4-9.4c-12.5-12.5-32.8-12.5-45.3 0s-12.5 32.8 0 45.3l160 160c12.5 12.5 32.8 12.5 45.3 0s12.5-32.8 0-45.3l-9.4-9.4 101.6-101.6c39-39 39-102.2 0-141.1s-102.2-39-141.1 0zM55.4 323.3c-15 15-23.4 35.4-23.4 56.6l0 42.4-26.6 39.9c-8.5 12.7-6.8 29.6 4 40.4s27.7 12.5 40.4 4l39.9-26.6 42.4 0c21.2 0 41.6-8.4 56.6-23.4l109.4-109.4-45.3-45.3-109.4 109.4c-3 3-7.1 4.7-11.3 4.7l-36.1 0 0-36.1c0-4.2 1.7-8.3 4.7-11.3l109.4-109.4-45.3-45.3-109.4 109.4z"/></svg>',forward:'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 640 640"><!--!Font Awesome Free v7.2.0 by @fontawesome - https://fontawesome.com License - https://fontawesome.com/license/free Copyright 2026 Fonticons, Inc.--><path fill="currentColor" d="M403.7 107.1C392.1 96 375 92.9 360.3 99.2C345.6 105.5 336 120 336 136L336 272.3L163.7 107.2C152.1 96 135 92.9 120.3 99.2C105.6 105.5 96 120 96 136L96 504C96 520 105.6 534.5 120.3 540.8C135 547.1 152.1 544 163.7 532.9L336 367.7L336 504C336 520 345.6 534.5 360.3 540.8C375 547.1 392.1 544 403.7 532.9L595.7 348.9C603.6 341.4 608 330.9 608 320C608 309.1 603.5 298.7 595.7 291.1L403.7 107.1z"/></svg>',file:'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 640 640"><!--!Font Awesome Free 7.1.0 by @fontawesome - https://fontawesome.com License - https://fontawesome.com/license/free Copyright 2026 Fonticons, Inc.--><path fill="currentColor" d="M192 64C156.7 64 128 92.7 128 128L128 512C128 547.3 156.7 576 192 576L448 576C483.3 576 512 547.3 512 512L512 234.5C512 217.5 505.3 201.2 493.3 189.2L386.7 82.7C374.7 70.7 358.5 64 341.5 64L192 64zM453.5 240L360 240C346.7 240 336 229.3 336 216L336 122.5L453.5 240z"/></svg>',"file-audio":'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 640 640"><!--!Font Awesome Free 7.1.0 by @fontawesome - https://fontawesome.com License - https://fontawesome.com/license/free Copyright 2026 Fonticons, Inc.--><path fill="currentColor" d="M128 128C128 92.7 156.7 64 192 64L341.5 64C358.5 64 374.8 70.7 386.8 82.7L493.3 189.3C505.3 201.3 512 217.6 512 234.6L512 512C512 547.3 483.3 576 448 576L192 576C156.7 576 128 547.3 128 512L128 128zM336 122.5L336 216C336 229.3 346.7 240 360 240L453.5 240L336 122.5zM389.8 307.7C380.7 301.4 368.3 303.6 362 312.7C355.7 321.8 357.9 334.2 367 340.5C390.9 357.2 406.4 384.8 406.4 416C406.4 447.2 390.8 474.9 367 491.5C357.9 497.8 355.7 510.3 362 519.3C368.3 528.3 380.8 530.6 389.8 524.3C423.9 500.5 446.4 460.8 446.4 416C446.4 371.2 424 331.5 389.8 307.7zM208 376C199.2 376 192 383.2 192 392L192 440C192 448.8 199.2 456 208 456L232 456L259.2 490C262.2 493.8 266.8 496 271.7 496L272 496C280.8 496 288 488.8 288 480L288 352C288 343.2 280.8 336 272 336L271.7 336C266.8 336 262.2 338.2 259.2 342L232 376L208 376zM336 448.2C336 458.9 346.5 466.4 354.9 459.8C367.8 449.5 376 433.7 376 416C376 398.3 367.8 382.5 354.9 372.2C346.5 365.5 336 373.1 336 383.8L336 448.3z"/></svg>',"file-code":'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 640 640"><!--!Font Awesome Free 7.1.0 by @fontawesome - https://fontawesome.com License - https://fontawesome.com/license/free Copyright 2026 Fonticons, Inc.--><path fill="currentColor" d="M128 128C128 92.7 156.7 64 192 64L341.5 64C358.5 64 374.8 70.7 386.8 82.7L493.3 189.3C505.3 201.3 512 217.6 512 234.6L512 512C512 547.3 483.3 576 448 576L192 576C156.7 576 128 547.3 128 512L128 128zM336 122.5L336 216C336 229.3 346.7 240 360 240L453.5 240L336 122.5zM282.2 359.6C290.8 349.5 289.7 334.4 279.6 325.8C269.5 317.2 254.4 318.3 245.8 328.4L197.8 384.4C190.1 393.4 190.1 406.6 197.8 415.6L245.8 471.6C254.4 481.7 269.6 482.8 279.6 474.2C289.6 465.6 290.8 450.4 282.2 440.4L247.6 400L282.2 359.6zM394.2 328.4C385.6 318.3 370.4 317.2 360.4 325.8C350.4 334.4 349.2 349.6 357.8 359.6L392.4 400L357.8 440.4C349.2 450.5 350.3 465.6 360.4 474.2C370.5 482.8 385.6 481.7 394.2 471.6L442.2 415.6C449.9 406.6 449.9 393.4 442.2 384.4L394.2 328.4z"/></svg>',"file-excel":'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 640 640"><!--!Font Awesome Free 7.1.0 by @fontawesome - https://fontawesome.com License - https://fontawesome.com/license/free Copyright 2026 Fonticons, Inc.--><path fill="currentColor" d="M128 128C128 92.7 156.7 64 192 64L341.5 64C358.5 64 374.8 70.7 386.8 82.7L493.3 189.3C505.3 201.3 512 217.6 512 234.6L512 512C512 547.3 483.3 576 448 576L192 576C156.7 576 128 547.3 128 512L128 128zM336 122.5L336 216C336 229.3 346.7 240 360 240L453.5 240L336 122.5zM292 330.7C284.6 319.7 269.7 316.7 258.7 324C247.7 331.3 244.7 346.3 252 357.3L291.2 416L252 474.7C244.6 485.7 247.6 500.6 258.7 508C269.8 515.4 284.6 512.4 292 501.3L320 459.3L348 501.3C355.4 512.3 370.3 515.3 381.3 508C392.3 500.7 395.3 485.7 388 474.7L348.8 416L388 357.3C395.4 346.3 392.4 331.4 381.3 324C370.2 316.6 355.4 319.6 348 330.7L320 372.7L292 330.7z"/></svg>',"file-image":'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 640 640"><!--!Font Awesome Free 7.1.0 by @fontawesome - https://fontawesome.com License - https://fontawesome.com/license/free Copyright 2026 Fonticons, Inc.--><path fill="currentColor" d="M128 128C128 92.7 156.7 64 192 64L341.5 64C358.5 64 374.8 70.7 386.8 82.7L493.3 189.3C505.3 201.3 512 217.6 512 234.6L512 512C512 547.3 483.3 576 448 576L192 576C156.7 576 128 547.3 128 512L128 128zM336 122.5L336 216C336 229.3 346.7 240 360 240L453.5 240L336 122.5zM256 320C256 302.3 241.7 288 224 288C206.3 288 192 302.3 192 320C192 337.7 206.3 352 224 352C241.7 352 256 337.7 256 320zM220.6 512L419.4 512C435.2 512 448 499.2 448 483.4C448 476.1 445.2 469 440.1 463.7L343.3 361.9C337.3 355.6 328.9 352 320.1 352L319.8 352C311 352 302.7 355.6 296.6 361.9L199.9 463.7C194.8 469 192 476.1 192 483.4C192 499.2 204.8 512 220.6 512z"/></svg>',"file-pdf":'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 640 640"><!--!Font Awesome Free 7.1.0 by @fontawesome - https://fontawesome.com License - https://fontawesome.com/license/free Copyright 2026 Fonticons, Inc.--><path fill="currentColor" d="M128 64C92.7 64 64 92.7 64 128L64 512C64 547.3 92.7 576 128 576L208 576L208 464C208 428.7 236.7 400 272 400L448 400L448 234.5C448 217.5 441.3 201.2 429.3 189.2L322.7 82.7C310.7 70.7 294.5 64 277.5 64L128 64zM389.5 240L296 240C282.7 240 272 229.3 272 216L272 122.5L389.5 240zM272 444C261 444 252 453 252 464L252 592C252 603 261 612 272 612C283 612 292 603 292 592L292 564L304 564C337.1 564 364 537.1 364 504C364 470.9 337.1 444 304 444L272 444zM304 524L292 524L292 484L304 484C315 484 324 493 324 504C324 515 315 524 304 524zM400 444C389 444 380 453 380 464L380 592C380 603 389 612 400 612L432 612C460.7 612 484 588.7 484 560L484 496C484 467.3 460.7 444 432 444L400 444zM420 572L420 484L432 484C438.6 484 444 489.4 444 496L444 560C444 566.6 438.6 572 432 572L420 572zM508 464L508 592C508 603 517 612 528 612C539 612 548 603 548 592L548 548L576 548C587 548 596 539 596 528C596 517 587 508 576 508L548 508L548 484L576 484C587 484 596 475 596 464C596 453 587 444 576 444L528 444C517 444 508 453 508 464z"/></svg>',"file-powerpoint":'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 640 640"><!--!Font Awesome Free 7.1.0 by @fontawesome - https://fontawesome.com License - https://fontawesome.com/license/free Copyright 2026 Fonticons, Inc.--><path fill="currentColor" d="M128 128C128 92.7 156.7 64 192 64L341.5 64C358.5 64 374.8 70.7 386.8 82.7L493.3 189.3C505.3 201.3 512 217.6 512 234.6L512 512C512 547.3 483.3 576 448 576L192 576C156.7 576 128 547.3 128 512L128 128zM336 122.5L336 216C336 229.3 346.7 240 360 240L453.5 240L336 122.5zM280 320C266.7 320 256 330.7 256 344L256 488C256 501.3 266.7 512 280 512C293.3 512 304 501.3 304 488L304 464L328 464C367.8 464 400 431.8 400 392C400 352.2 367.8 320 328 320L280 320zM328 416L304 416L304 368L328 368C341.3 368 352 378.7 352 392C352 405.3 341.3 416 328 416z"/></svg>',"file-video":'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 640 640"><!--!Font Awesome Free 7.1.0 by @fontawesome - https://fontawesome.com License - https://fontawesome.com/license/free Copyright 2026 Fonticons, Inc.--><path fill="currentColor" d="M128 128C128 92.7 156.7 64 192 64L341.5 64C358.5 64 374.8 70.7 386.8 82.7L493.3 189.3C505.3 201.3 512 217.6 512 234.6L512 512C512 547.3 483.3 576 448 576L192 576C156.7 576 128 547.3 128 512L128 128zM336 122.5L336 216C336 229.3 346.7 240 360 240L453.5 240L336 122.5zM208 368L208 464C208 481.7 222.3 496 240 496L336 496C353.7 496 368 481.7 368 464L368 440L403 475C406.2 478.2 410.5 480 415 480C424.4 480 432 472.4 432 463L432 368.9C432 359.5 424.4 351.9 415 351.9C410.5 351.9 406.2 353.7 403 356.9L368 391.9L368 367.9C368 350.2 353.7 335.9 336 335.9L240 335.9C222.3 335.9 208 350.2 208 367.9z"/></svg>',"file-word":'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 640 640"><!--!Font Awesome Free 7.1.0 by @fontawesome - https://fontawesome.com License - https://fontawesome.com/license/free Copyright 2026 Fonticons, Inc.--><path fill="currentColor" d="M128 128C128 92.7 156.7 64 192 64L341.5 64C358.5 64 374.8 70.7 386.8 82.7L493.3 189.3C505.3 201.3 512 217.6 512 234.6L512 512C512 547.3 483.3 576 448 576L192 576C156.7 576 128 547.3 128 512L128 128zM336 122.5L336 216C336 229.3 346.7 240 360 240L453.5 240L336 122.5zM263.4 338.8C260.5 325.9 247.7 317.7 234.8 320.6C221.9 323.5 213.7 336.3 216.6 349.2L248.6 493.2C250.9 503.7 260 511.4 270.8 512C281.6 512.6 291.4 505.9 294.8 495.6L320 419.9L345.2 495.6C348.6 505.8 358.4 512.5 369.2 512C380 511.5 389.1 503.8 391.4 493.2L423.4 349.2C426.3 336.3 418.1 323.4 405.2 320.6C392.3 317.8 379.4 325.9 376.6 338.8L363.4 398.2L342.8 336.4C339.5 326.6 330.4 320 320 320C309.6 320 300.5 326.6 297.2 336.4L276.6 398.2L263.4 338.8z"/></svg>',"file-zipper":'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 640 640"><!--!Font Awesome Free 7.1.0 by @fontawesome - https://fontawesome.com License - https://fontawesome.com/license/free Copyright 2026 Fonticons, Inc.--><path fill="currentColor" d="M128 128C128 92.7 156.7 64 192 64L341.5 64C358.5 64 374.8 70.7 386.8 82.7L493.3 189.3C505.3 201.3 512 217.6 512 234.6L512 512C512 547.3 483.3 576 448 576L192 576C156.7 576 128 547.3 128 512L128 128zM336 122.5L336 216C336 229.3 346.7 240 360 240L453.5 240L336 122.5zM192 136C192 149.3 202.7 160 216 160L264 160C277.3 160 288 149.3 288 136C288 122.7 277.3 112 264 112L216 112C202.7 112 192 122.7 192 136zM192 232C192 245.3 202.7 256 216 256L264 256C277.3 256 288 245.3 288 232C288 218.7 277.3 208 264 208L216 208C202.7 208 192 218.7 192 232zM256 304L224 304C206.3 304 192 318.3 192 336L192 384C192 410.5 213.5 432 240 432C266.5 432 288 410.5 288 384L288 336C288 318.3 273.7 304 256 304zM240 368C248.8 368 256 375.2 256 384C256 392.8 248.8 400 240 400C231.2 400 224 392.8 224 384C224 375.2 231.2 368 240 368z"/></svg>',"forward-step":'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 384 512"><!--!Font Awesome Free v7.2.0 by @fontawesome - https://fontawesome.com License - https://fontawesome.com/license/free Copyright 2026 Fonticons, Inc.--><path fill="currentColor" d="M21 36.8c12.9-7 28.7-6.3 41 1.8L320 208.1 320 64c0-17.7 14.3-32 32-32s32 14.3 32 32l0 384c0 17.7-14.3 32-32 32s-32-14.3-32-32l0-144.1-258 169.6c-12.3 8.1-28 8.8-41 1.8S0 454.7 0 440L0 72C0 57.3 8.1 43.8 21 36.8z"/></svg>',gauge:'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 512 512"><!--!Font Awesome Free v7.2.0 by @fontawesome - https://fontawesome.com License - https://fontawesome.com/license/free Copyright 2026 Fonticons, Inc.--><path fill="currentColor" d="M0 256a256 256 0 1 1 512 0 256 256 0 1 1 -512 0zm320 96c0-26.9-16.5-49.9-40-59.3L280 120c0-13.3-10.7-24-24-24s-24 10.7-24 24l0 172.7c-23.5 9.5-40 32.5-40 59.3 0 35.3 28.7 64 64 64s64-28.7 64-64zM144 176a32 32 0 1 0 0-64 32 32 0 1 0 0 64zm-16 80a32 32 0 1 0 -64 0 32 32 0 1 0 64 0zm288 32a32 32 0 1 0 0-64 32 32 0 1 0 0 64zM400 144a32 32 0 1 0 -64 0 32 32 0 1 0 64 0z"/></svg>',gear:'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 640 640"><!--!Font Awesome Free v7.2.0 by @fontawesome - https://fontawesome.com License - https://fontawesome.com/license/free Copyright 2026 Fonticons, Inc.--><path fill="currentColor" d="M259.1 73.5C262.1 58.7 275.2 48 290.4 48L350.2 48C365.4 48 378.5 58.7 381.5 73.5L396 143.5C410.1 149.5 423.3 157.2 435.3 166.3L503.1 143.8C517.5 139 533.3 145 540.9 158.2L570.8 210C578.4 223.2 575.7 239.8 564.3 249.9L511 297.3C511.9 304.7 512.3 312.3 512.3 320C512.3 327.7 511.8 335.3 511 342.7L564.4 390.2C575.8 400.3 578.4 417 570.9 430.1L541 481.9C533.4 495 517.6 501.1 503.2 496.3L435.4 473.8C423.3 482.9 410.1 490.5 396.1 496.6L381.7 566.5C378.6 581.4 365.5 592 350.4 592L290.6 592C275.4 592 262.3 581.3 259.3 566.5L244.9 496.6C230.8 490.6 217.7 482.9 205.6 473.8L137.5 496.3C123.1 501.1 107.3 495.1 99.7 481.9L69.8 430.1C62.2 416.9 64.9 400.3 76.3 390.2L129.7 342.7C128.8 335.3 128.4 327.7 128.4 320C128.4 312.3 128.9 304.7 129.7 297.3L76.3 249.8C64.9 239.7 62.3 223 69.8 209.9L99.7 158.1C107.3 144.9 123.1 138.9 137.5 143.7L205.3 166.2C217.4 157.1 230.6 149.5 244.6 143.4L259.1 73.5zM320.3 400C364.5 399.8 400.2 363.9 400 319.7C399.8 275.5 363.9 239.8 319.7 240C275.5 240.2 239.8 276.1 240 320.3C240.2 364.5 276.1 400.2 320.3 400z"/></svg>',"grip-vertical":'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 320 512"><!--! Font Awesome Free 7.0.0 by @fontawesome - https://fontawesome.com License - https://fontawesome.com/license/free Copyright 2025 Fonticons, Inc. --><path fill="currentColor" d="M128 40c0-22.1-17.9-40-40-40L40 0C17.9 0 0 17.9 0 40L0 88c0 22.1 17.9 40 40 40l48 0c22.1 0 40-17.9 40-40l0-48zm0 192c0-22.1-17.9-40-40-40l-48 0c-22.1 0-40 17.9-40 40l0 48c0 22.1 17.9 40 40 40l48 0c22.1 0 40-17.9 40-40l0-48zM0 424l0 48c0 22.1 17.9 40 40 40l48 0c22.1 0 40-17.9 40-40l0-48c0-22.1-17.9-40-40-40l-48 0c-22.1 0-40 17.9-40 40zM320 40c0-22.1-17.9-40-40-40L232 0c-22.1 0-40 17.9-40 40l0 48c0 22.1 17.9 40 40 40l48 0c22.1 0 40-17.9 40-40l0-48zM192 232l0 48c0 22.1 17.9 40 40 40l48 0c22.1 0 40-17.9 40-40l0-48c0-22.1-17.9-40-40-40l-48 0c-22.1 0-40 17.9-40 40zM320 424c0-22.1-17.9-40-40-40l-48 0c-22.1 0-40 17.9-40 40l0 48c0 22.1 17.9 40 40 40l48 0c22.1 0 40-17.9 40-40l0-48z"/></svg>',indeterminate:'<svg part="indeterminate-icon" class="icon" viewBox="0 0 16 16"><g stroke="none" stroke-width="1" fill="none" fill-rule="evenodd" stroke-linecap="round"><g stroke="currentColor" stroke-width="2"><g transform="translate(2.285714 6.857143)"><path d="M10.2857143,1.14285714 L1.14285714,1.14285714"/></g></g></g></svg>',minus:'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 448 512"><!--! Font Awesome Free 7.0.0 by @fontawesome - https://fontawesome.com License - https://fontawesome.com/license/free Copyright 2025 Fonticons, Inc. --><path fill="currentColor" d="M0 256c0-17.7 14.3-32 32-32l384 0c17.7 0 32 14.3 32 32s-14.3 32-32 32L32 288c-17.7 0-32-14.3-32-32z"/></svg>',pause:'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 384 512"><!--! Font Awesome Free 7.0.0 by @fontawesome - https://fontawesome.com License - https://fontawesome.com/license/free Copyright 2025 Fonticons, Inc. --><path fill="currentColor" d="M48 32C21.5 32 0 53.5 0 80L0 432c0 26.5 21.5 48 48 48l64 0c26.5 0 48-21.5 48-48l0-352c0-26.5-21.5-48-48-48L48 32zm224 0c-26.5 0-48 21.5-48 48l0 352c0 26.5 21.5 48 48 48l64 0c26.5 0 48-21.5 48-48l0-352c0-26.5-21.5-48-48-48l-64 0z"/></svg>',"picture-in-picture":'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 512 512"><!--!Font Awesome Free v7.2.0 by @fontawesome - https://fontawesome.com License - https://fontawesome.com/license/free Copyright 2026 Fonticons, Inc.--><path fill="currentColor" d="M448 32c35.3 0 64 28.7 64 64l0 112-64 0 0-112-384 0 0 320 144 0 0 64-144 0-6.5-.3c-30.1-3.1-54.1-27-57.1-57.1L0 416 0 96C0 62.9 25.2 35.6 57.5 32.3L64 32 448 32zm16 224c26.5 0 48 21.5 48 48l0 128c0 26.5-21.5 48-48 48l-160 0c-26.5 0-48-21.5-48-48l0-128c0-26.5 21.5-48 48-48l160 0z"/></svg>',play:'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 448 512"><!--! Font Awesome Free 7.0.0 by @fontawesome - https://fontawesome.com License - https://fontawesome.com/license/free Copyright 2025 Fonticons, Inc. --><path fill="currentColor" d="M91.2 36.9c-12.4-6.8-27.4-6.5-39.6 .7S32 57.9 32 72l0 368c0 14.1 7.5 27.2 19.6 34.4s27.2 7.5 39.6 .7l336-184c12.8-7 20.8-20.5 20.8-35.1s-8-28.1-20.8-35.1l-336-184z"/></svg>',"play-circle":'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 512 512"><!--!Font Awesome Free v7.2.0 by @fontawesome - https://fontawesome.com License - https://fontawesome.com/license/free Copyright 2026 Fonticons, Inc.--><path fill="currentColor" d="M0 256a256 256 0 1 1 512 0 256 256 0 1 1 -512 0zM188.3 147.1c-7.6 4.2-12.3 12.3-12.3 20.9l0 176c0 8.7 4.7 16.7 12.3 20.9s16.8 4.1 24.3-.5l144-88c7.1-4.4 11.5-12.1 11.5-20.5s-4.4-16.1-11.5-20.5l-144-88c-7.4-4.5-16.7-4.7-24.3-.5z"/></svg>',plus:'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 640 640"><!--!Font Awesome Free 7.1.0 by @fontawesome - https://fontawesome.com License - https://fontawesome.com/license/free Copyright 2026 Fonticons, Inc.--><path fill="currentColor" d="M352 128C352 110.3 337.7 96 320 96C302.3 96 288 110.3 288 128L288 288L128 288C110.3 288 96 302.3 96 320C96 337.7 110.3 352 128 352L288 352L288 512C288 529.7 302.3 544 320 544C337.7 544 352 529.7 352 512L352 352L512 352C529.7 352 544 337.7 544 320C544 302.3 529.7 288 512 288L352 288L352 128z"/></svg>',star:'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 576 512"><!--! Font Awesome Free 7.0.0 by @fontawesome - https://fontawesome.com License - https://fontawesome.com/license/free Copyright 2025 Fonticons, Inc. --><path fill="currentColor" d="M309.5-18.9c-4.1-8-12.4-13.1-21.4-13.1s-17.3 5.1-21.4 13.1L193.1 125.3 33.2 150.7c-8.9 1.4-16.3 7.7-19.1 16.3s-.5 18 5.8 24.4l114.4 114.5-25.2 159.9c-1.4 8.9 2.3 17.9 9.6 23.2s16.9 6.1 25 2L288.1 417.6 432.4 491c8 4.1 17.7 3.3 25-2s11-14.2 9.6-23.2L441.7 305.9 556.1 191.4c6.4-6.4 8.6-15.8 5.8-24.4s-10.1-14.9-19.1-16.3L383 125.3 309.5-18.9z"/></svg>',upload:'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 640 640"><!--!Font Awesome Free 7.1.0 by @fontawesome - https://fontawesome.com License - https://fontawesome.com/license/free Copyright 2026 Fonticons, Inc.--><path fill="currentColor" d="M352 173.3L352 384C352 401.7 337.7 416 320 416C302.3 416 288 401.7 288 384L288 173.3L246.6 214.7C234.1 227.2 213.8 227.2 201.3 214.7C188.8 202.2 188.8 181.9 201.3 169.4L297.3 73.4C309.8 60.9 330.1 60.9 342.6 73.4L438.6 169.4C451.1 181.9 451.1 202.2 438.6 214.7C426.1 227.2 405.8 227.2 393.3 214.7L352 173.3zM320 464C364.2 464 400 428.2 400 384L480 384C515.3 384 544 412.7 544 448L544 480C544 515.3 515.3 544 480 544L160 544C124.7 544 96 515.3 96 480L96 448C96 412.7 124.7 384 160 384L240 384C240 428.2 275.8 464 320 464zM464 488C477.3 488 488 477.3 488 464C488 450.7 477.3 440 464 440C450.7 440 440 450.7 440 464C440 477.3 450.7 488 464 488z"/></svg>',user:'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 448 512"><!--! Font Awesome Free 7.0.0 by @fontawesome - https://fontawesome.com License - https://fontawesome.com/license/free Copyright 2025 Fonticons, Inc. --><path fill="currentColor" d="M224 248a120 120 0 1 0 0-240 120 120 0 1 0 0 240zm-29.7 56C95.8 304 16 383.8 16 482.3 16 498.7 29.3 512 45.7 512l356.6 0c16.4 0 29.7-13.3 29.7-29.7 0-98.5-79.8-178.3-178.3-178.3l-59.4 0z"/></svg>',volume:'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 512 512"><!--!Font Awesome Free v7.2.0 by @fontawesome - https://fontawesome.com License - https://fontawesome.com/license/free Copyright 2026 Fonticons, Inc.--><path fill="currentColor" d="M48 352l48 0 134.1 119.2c6.4 5.7 14.6 8.8 23.1 8.8 19.2 0 34.8-15.6 34.8-34.8l0-378.4c0-19.2-15.6-34.8-34.8-34.8-8.5 0-16.7 3.1-23.1 8.8L96 160 48 160c-26.5 0-48 21.5-48 48l0 96c0 26.5 21.5 48 48 48zM441.1 107c-10.3-8.4-25.4-6.8-33.8 3.5s-6.8 25.4 3.5 33.8C443.3 170.7 464 210.9 464 256s-20.7 85.3-53.2 111.8c-10.3 8.4-11.8 23.5-3.5 33.8s23.5 11.8 33.8 3.5c43.2-35.2 70.9-88.9 70.9-149s-27.7-113.8-70.9-149zm-60.5 74.5c-10.3-8.4-25.4-6.8-33.8 3.5s-6.8 25.4 3.5 33.8C361.1 227.6 368 241 368 256s-6.9 28.4-17.7 37.3c-10.3 8.4-11.8 23.5-3.5 33.8s23.5 11.8 33.8 3.5C402.1 312.9 416 286.1 416 256s-13.9-56.9-35.5-74.5z"/></svg>',"volume-low":'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 448 512"><!--!Font Awesome Free v7.2.0 by @fontawesome - https://fontawesome.com License - https://fontawesome.com/license/free Copyright 2026 Fonticons, Inc.--><path fill="currentColor" d="M48 352l48 0 134.1 119.2c6.4 5.7 14.6 8.8 23.1 8.8 19.2 0 34.8-15.6 34.8-34.8l0-378.4c0-19.2-15.6-34.8-34.8-34.8-8.5 0-16.7 3.1-23.1 8.8L96 160 48 160c-26.5 0-48 21.5-48 48l0 96c0 26.5 21.5 48 48 48zM380.6 181.5c-10.3-8.4-25.4-6.8-33.8 3.5s-6.8 25.4 3.5 33.8C361.1 227.6 368 241 368 256s-6.9 28.4-17.7 37.3c-10.3 8.4-11.8 23.5-3.5 33.8s23.5 11.8 33.8 3.5C402.1 312.9 416 286.1 416 256s-13.9-56.9-35.5-74.5z"/></svg>',"volume-xmark":'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 576 512"><!--!Font Awesome Free v7.2.0 by @fontawesome - https://fontawesome.com License - https://fontawesome.com/license/free Copyright 2026 Fonticons, Inc.--><path fill="currentColor" d="M48 352l48 0 134.1 119.2c6.4 5.7 14.6 8.8 23.1 8.8 19.2 0 34.8-15.6 34.8-34.8l0-378.4c0-19.2-15.6-34.8-34.8-34.8-8.5 0-16.7 3.1-23.1 8.8L96 160 48 160c-26.5 0-48 21.5-48 48l0 96c0 26.5 21.5 48 48 48zM367 175c-9.4 9.4-9.4 24.6 0 33.9l47 47-47 47c-9.4 9.4-9.4 24.6 0 33.9s24.6 9.4 33.9 0l47-47 47 47c9.4 9.4 24.6 9.4 33.9 0s9.4-24.6 0-33.9l-47-47 47-47c9.4-9.4 9.4-24.6 0-33.9s-24.6-9.4-33.9 0l-47 47-47-47c-9.4-9.4-24.6-9.4-33.9 0z"/></svg>',xmark:'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 384 512"><!--! Font Awesome Free 7.0.0 by @fontawesome - https://fontawesome.com License - https://fontawesome.com/license/free Copyright 2025 Fonticons, Inc. --><path fill="currentColor" d="M55.1 73.4c-12.5-12.5-32.8-12.5-45.3 0s-12.5 32.8 0 45.3L147.2 256 9.9 393.4c-12.5 12.5-12.5 32.8 0 45.3s32.8 12.5 45.3 0L192.5 301.3 329.9 438.6c12.5 12.5 32.8 12.5 45.3 0s12.5-32.8 0-45.3L237.8 256 375.1 118.6c12.5-12.5 12.5-32.8 0-45.3s-32.8-12.5-45.3 0L192.5 210.7 55.1 73.4z"/></svg>'},regular:{calendar:'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 640 640"><!--!Font Awesome Free v7.2.0 by @fontawesome - https://fontawesome.com License - https://fontawesome.com/license/free Copyright 2026 Fonticons, Inc.--><path d="M216 64C229.3 64 240 74.7 240 88L240 128L400 128L400 88C400 74.7 410.7 64 424 64C437.3 64 448 74.7 448 88L448 128L480 128C515.3 128 544 156.7 544 192L544 480C544 515.3 515.3 544 480 544L160 544C124.7 544 96 515.3 96 480L96 192C96 156.7 124.7 128 160 128L192 128L192 88C192 74.7 202.7 64 216 64zM216 176L160 176C151.2 176 144 183.2 144 192L144 240L496 240L496 192C496 183.2 488.8 176 480 176L216 176zM144 288L144 480C144 488.8 151.2 496 160 496L480 496C488.8 496 496 488.8 496 480L496 288L144 288z"/></svg>',"circle-question":'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 512 512"><!--! Font Awesome Free 7.0.0 by @fontawesome - https://fontawesome.com License - https://fontawesome.com/license/free Copyright 2025 Fonticons, Inc. --><path fill="currentColor" d="M464 256a208 208 0 1 0 -416 0 208 208 0 1 0 416 0zM0 256a256 256 0 1 1 512 0 256 256 0 1 1 -512 0zm256-80c-17.7 0-32 14.3-32 32 0 13.3-10.7 24-24 24s-24-10.7-24-24c0-44.2 35.8-80 80-80s80 35.8 80 80c0 47.2-36 67.2-56 74.5l0 3.8c0 13.3-10.7 24-24 24s-24-10.7-24-24l0-8.1c0-20.5 14.8-35.2 30.1-40.2 6.4-2.1 13.2-5.5 18.2-10.3 4.3-4.2 7.7-10 7.7-19.6 0-17.7-14.3-32-32-32zM224 368a32 32 0 1 1 64 0 32 32 0 1 1 -64 0z"/></svg>',"circle-xmark":'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 512 512"><!--! Font Awesome Free 7.0.0 by @fontawesome - https://fontawesome.com License - https://fontawesome.com/license/free Copyright 2025 Fonticons, Inc. --><path fill="currentColor" d="M256 48a208 208 0 1 1 0 416 208 208 0 1 1 0-416zm0 464a256 256 0 1 0 0-512 256 256 0 1 0 0 512zM167 167c-9.4 9.4-9.4 24.6 0 33.9l55 55-55 55c-9.4 9.4-9.4 24.6 0 33.9s24.6 9.4 33.9 0l55-55 55 55c9.4 9.4 24.6 9.4 33.9 0s9.4-24.6 0-33.9l-55-55 55-55c9.4-9.4 9.4-24.6 0-33.9s-24.6-9.4-33.9 0l-55 55-55-55c-9.4-9.4-24.6-9.4-33.9 0z"/></svg>',clock:'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 640 640"><!--!Font Awesome Free v7.2.0 by @fontawesome - https://fontawesome.com License - https://fontawesome.com/license/free Copyright 2026 Fonticons, Inc.--><path d="M528 320C528 434.9 434.9 528 320 528C205.1 528 112 434.9 112 320C112 205.1 205.1 112 320 112C434.9 112 528 205.1 528 320zM64 320C64 461.4 178.6 576 320 576C461.4 576 576 461.4 576 320C576 178.6 461.4 64 320 64C178.6 64 64 178.6 64 320zM296 184L296 320C296 328 300 335.5 306.7 340L402.7 404C413.7 411.4 428.6 408.4 436 397.3C443.4 386.2 440.4 371.4 429.3 364L344 307.2L344 184C344 170.7 333.3 160 320 160C306.7 160 296 170.7 296 184z"/></svg>',copy:'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 448 512"><!--! Font Awesome Free 7.0.0 by @fontawesome - https://fontawesome.com License - https://fontawesome.com/license/free Copyright 2025 Fonticons, Inc. --><path fill="currentColor" d="M384 336l-192 0c-8.8 0-16-7.2-16-16l0-256c0-8.8 7.2-16 16-16l133.5 0c4.2 0 8.3 1.7 11.3 4.7l58.5 58.5c3 3 4.7 7.1 4.7 11.3L400 320c0 8.8-7.2 16-16 16zM192 384l192 0c35.3 0 64-28.7 64-64l0-197.5c0-17-6.7-33.3-18.7-45.3L370.7 18.7C358.7 6.7 342.5 0 325.5 0L192 0c-35.3 0-64 28.7-64 64l0 256c0 35.3 28.7 64 64 64zM64 128c-35.3 0-64 28.7-64 64L0 448c0 35.3 28.7 64 64 64l192 0c35.3 0 64-28.7 64-64l0-16-48 0 0 16c0 8.8-7.2 16-16 16L64 464c-8.8 0-16-7.2-16-16l0-256c0-8.8 7.2-16 16-16l16 0 0-48-16 0z"/></svg>',eye:'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 576 512"><!--! Font Awesome Free 7.0.0 by @fontawesome - https://fontawesome.com License - https://fontawesome.com/license/free Copyright 2025 Fonticons, Inc. --><path fill="currentColor" d="M288 80C222.8 80 169.2 109.6 128.1 147.7 89.6 183.5 63 226 49.4 256 63 286 89.6 328.5 128.1 364.3 169.2 402.4 222.8 432 288 432s118.8-29.6 159.9-67.7C486.4 328.5 513 286 526.6 256 513 226 486.4 183.5 447.9 147.7 406.8 109.6 353.2 80 288 80zM95.4 112.6C142.5 68.8 207.2 32 288 32s145.5 36.8 192.6 80.6c46.8 43.5 78.1 95.4 93 131.1 3.3 7.9 3.3 16.7 0 24.6-14.9 35.7-46.2 87.7-93 131.1-47.1 43.7-111.8 80.6-192.6 80.6S142.5 443.2 95.4 399.4c-46.8-43.5-78.1-95.4-93-131.1-3.3-7.9-3.3-16.7 0-24.6 14.9-35.7 46.2-87.7 93-131.1zM288 336c44.2 0 80-35.8 80-80 0-29.6-16.1-55.5-40-69.3-1.4 59.7-49.6 107.9-109.3 109.3 13.8 23.9 39.7 40 69.3 40zm-79.6-88.4c2.5 .3 5 .4 7.6 .4 35.3 0 64-28.7 64-64 0-2.6-.2-5.1-.4-7.6-37.4 3.9-67.2 33.7-71.1 71.1zm45.6-115c10.8-3 22.2-4.5 33.9-4.5 8.8 0 17.5 .9 25.8 2.6 .3 .1 .5 .1 .8 .2 57.9 12.2 101.4 63.7 101.4 125.2 0 70.7-57.3 128-128 128-61.6 0-113-43.5-125.2-101.4-1.8-8.6-2.8-17.5-2.8-26.6 0-11 1.4-21.8 4-32 .2-.7 .3-1.3 .5-1.9 11.9-43.4 46.1-77.6 89.5-89.5z"/></svg>',"eye-slash":'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 576 512"><!--! Font Awesome Free 7.0.0 by @fontawesome - https://fontawesome.com License - https://fontawesome.com/license/free Copyright 2025 Fonticons, Inc. --><path fill="currentColor" d="M41-24.9c-9.4-9.4-24.6-9.4-33.9 0S-2.3-.3 7 9.1l528 528c9.4 9.4 24.6 9.4 33.9 0s9.4-24.6 0-33.9l-96.4-96.4c2.7-2.4 5.4-4.8 8-7.2 46.8-43.5 78.1-95.4 93-131.1 3.3-7.9 3.3-16.7 0-24.6-14.9-35.7-46.2-87.7-93-131.1-47.1-43.7-111.8-80.6-192.6-80.6-56.8 0-105.6 18.2-146 44.2L41-24.9zM176.9 111.1c32.1-18.9 69.2-31.1 111.1-31.1 65.2 0 118.8 29.6 159.9 67.7 38.5 35.7 65.1 78.3 78.6 108.3-13.6 30-40.2 72.5-78.6 108.3-3.1 2.8-6.2 5.6-9.4 8.4L393.8 328c14-20.5 22.2-45.3 22.2-72 0-70.7-57.3-128-128-128-26.7 0-51.5 8.2-72 22.2l-39.1-39.1zm182 182l-108-108c11.1-5.8 23.7-9.1 37.1-9.1 44.2 0 80 35.8 80 80 0 13.4-3.3 26-9.1 37.1zM103.4 173.2l-34-34c-32.6 36.8-55 75.8-66.9 104.5-3.3 7.9-3.3 16.7 0 24.6 14.9 35.7 46.2 87.7 93 131.1 47.1 43.7 111.8 80.6 192.6 80.6 37.3 0 71.2-7.9 101.5-20.6L352.2 422c-20 6.4-41.4 10-64.2 10-65.2 0-118.8-29.6-159.9-67.7-38.5-35.7-65.1-78.3-78.6-108.3 10.4-23.1 28.6-53.6 54-82.8z"/></svg>',star:'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 576 512"><!--! Font Awesome Free 7.0.0 by @fontawesome - https://fontawesome.com License - https://fontawesome.com/license/free Copyright 2025 Fonticons, Inc. --><path fill="currentColor" d="M288.1-32c9 0 17.3 5.1 21.4 13.1L383 125.3 542.9 150.7c8.9 1.4 16.3 7.7 19.1 16.3s.5 18-5.8 24.4L441.7 305.9 467 465.8c1.4 8.9-2.3 17.9-9.6 23.2s-17 6.1-25 2L288.1 417.6 143.8 491c-8 4.1-17.7 3.3-25-2s-11-14.2-9.6-23.2L134.4 305.9 20 191.4c-6.4-6.4-8.6-15.8-5.8-24.4s10.1-14.9 19.1-16.3l159.9-25.4 73.6-144.2c4.1-8 12.4-13.1 21.4-13.1zm0 76.8L230.3 158c-3.5 6.8-10 11.6-17.6 12.8l-125.5 20 89.8 89.9c5.4 5.4 7.9 13.1 6.7 20.7l-19.8 125.5 113.3-57.6c6.8-3.5 14.9-3.5 21.8 0l113.3 57.6-19.8-125.5c-1.2-7.6 1.3-15.3 6.7-20.7l89.8-89.9-125.5-20c-7.6-1.2-14.1-6-17.6-12.8L288.1 44.8z"/></svg>'}},Hr={name:"system",resolver:(t,e="classic",n="solid")=>{let o=Gt[n][t]??Gt.regular[t]??Gt.regular["circle-question"];return o?Wr(o):""}},Xr=Hr;var Kr="classic",Yr=[jr,Xr],hi=new Set;function Gr(t){hi.add(t)}function Jr(t){hi.delete(t)}function Jt(t){return Yr.find(e=>e.name===t)}function Zr(){return Kr}const Qr=(t,e)=>t?._$litType$!==void 0,ta=t=>t.strings===void 0,ea={},na=(t,e=ea)=>t._$AH=e;var at=Symbol(),kt=Symbol(),Zt,Qt=new Map,S=class extends G{constructor(){super(...arguments),this.svg=null,this.autoWidth=!1,this.swapOpacity=!1,this.label="",this.library="default",this.rotate=0,this.resolveIcon=async(t,e)=>{let n;if(e?.spriteSheet){this.hasUpdated||await this.updateComplete,this.svg=z`<svg part="svg">
        <use part="use" href="${t}"></use>
      </svg>`,await this.updateComplete;const i=this.shadowRoot.querySelector("[part='svg']");return typeof e.mutator=="function"&&e.mutator(i,this),this.svg}try{if(n=await fetch(t,{mode:"cors"}),!n.ok)return n.status===410?at:kt}catch{return kt}try{const i=document.createElement("div");i.innerHTML=await n.text();const o=i.firstElementChild;if(o?.tagName?.toLowerCase()!=="svg")return at;Zt||(Zt=new DOMParser);const a=Zt.parseFromString(o.outerHTML,"text/html").body.querySelector("svg");return a?(a.part.add("svg"),document.adoptNode(a)):at}catch{return at}}}connectedCallback(){super.connectedCallback(),Gr(this)}firstUpdated(t){super.firstUpdated(t),this.hasAttribute("rotate")&&this.style.setProperty("--rotate-angle",`${this.rotate}deg`),this.setIcon()}disconnectedCallback(){super.disconnectedCallback(),Jr(this)}async getIconSource(){const t=Jt(this.library),e=this.family||Zr();if(this.name&&t){const n=this.canvas==="auto"||this.autoWidth;let i;try{i=await t.resolver(this.name,e,this.variant,n)}catch{i=void 0}return{url:i,fromLibrary:!0}}return{url:this.src,fromLibrary:!1}}handleLabelChange(){typeof this.label=="string"&&this.label.length>0?(this.setAttribute("role","img"),this.setAttribute("aria-label",this.label),this.removeAttribute("aria-hidden")):(this.removeAttribute("role"),this.removeAttribute("aria-label"),this.setAttribute("aria-hidden","true"))}async setIcon(){const{url:t,fromLibrary:e}=await this.getIconSource(),n=e?Jt(this.library):void 0;if(!t){this.svg=null;return}let i=Qt.get(t);i||(i=this.resolveIcon(t,n),Qt.set(t,i));const o=await i;o===kt&&Qt.delete(t);const r=await this.getIconSource();if(t===r.url){if(Qr(o)){this.svg=o;return}switch(o){case kt:case at:this.svg=null,this.dispatchEvent(new Ir);break;default:this.svg=o.cloneNode(!0),n?.mutator?.(this.svg,this),this.dispatchEvent(new Rr)}}}willUpdate(t){return this.style||this.setStyleProperty("--rotate-angle",`${this.rotate}deg`),super.willUpdate(t)}updated(t){super.updated(t);const e=Jt(this.library);this.hasAttribute("rotate")&&this.style.setProperty("--rotate-angle",`${this.rotate}deg`);const n=this.shadowRoot?.querySelector("svg");n&&e?.mutator?.(n,this)}render(){return this.hasUpdated?this.svg:z`<svg part="svg" width="16" height="16" viewBox="0 0 16 16"></svg>`}};S.css=Nr;d([jt()],S.prototype,"svg",2);d([h({reflect:!0})],S.prototype,"name",2);d([h({reflect:!0})],S.prototype,"family",2);d([h({reflect:!0})],S.prototype,"variant",2);d([h({reflect:!0})],S.prototype,"canvas",2);d([h({attribute:"auto-width",type:Boolean,reflect:!0})],S.prototype,"autoWidth",2);d([h({attribute:"swap-opacity",type:Boolean,reflect:!0})],S.prototype,"swapOpacity",2);d([h()],S.prototype,"src",2);d([h()],S.prototype,"label",2);d([h({reflect:!0})],S.prototype,"library",2);d([h({type:Number,reflect:!0})],S.prototype,"rotate",2);d([h({type:String,reflect:!0})],S.prototype,"flip",2);d([h({type:String,reflect:!0})],S.prototype,"animation",2);d([D("label")],S.prototype,"handleLabelChange",1);d([D(["family","name","library","variant","src","autoWidth","canvas","swapOpacity"],{waitUntilFirstUpdate:!0})],S.prototype,"setIcon",1);S=d([Lt("wa-icon")],S);var ia=class{constructor(t,e){this.element=t,this.callback=e}start(...t){this.observer??(this.observer=new ResizeObserver(()=>this.check())),this.observer.observe(this.element);for(const e of t)this.observer.observe(e);this.initialCheckHandle??(this.initialCheckHandle=requestAnimationFrame(()=>{this.initialCheckHandle=void 0,this.check()}))}stop(){this.initialCheckHandle!==void 0&&(cancelAnimationFrame(this.initialCheckHandle),this.initialCheckHandle=void 0),this.observer?.disconnect()}check(){this.callback(this.element.getClientRects().length>0)}};var se=new Set;function oa(){const t=document.documentElement.clientWidth;return Math.abs(window.innerWidth-t)}function ra(){const t=Number(getComputedStyle(document.body).paddingRight.replace(/px/,""));return isNaN(t)||!t?0:t}function te(t){if(se.add(t),!document.documentElement.classList.contains("wa-scroll-lock")){const e=oa()+ra();let n=getComputedStyle(document.documentElement).scrollbarGutter;(!n||n==="auto")&&(n="stable"),e<2&&(n=""),document.documentElement.style.setProperty("--wa-scroll-lock-gutter",n),document.documentElement.classList.add("wa-scroll-lock"),document.documentElement.style.setProperty("--wa-scroll-lock-size",`${e}px`)}}function ee(t){se.delete(t),se.size===0&&(document.documentElement.classList.remove("wa-scroll-lock"),document.documentElement.style.removeProperty("--wa-scroll-lock-size"))}function aa(t){return t.split(" ").map(e=>e.trim()).filter(e=>e!=="")}var sa=class extends Event{constructor(){super("wa-show",{bubbles:!0,cancelable:!0,composed:!0})}};var la=class extends Event{constructor(t){super("wa-hide",{bubbles:!0,cancelable:!0,composed:!0}),this.detail=t}};var ca=class extends Event{constructor(){super("wa-after-show",{bubbles:!0,cancelable:!1,composed:!0})}};var da=class extends Event{constructor(){super("wa-after-hide",{bubbles:!0,cancelable:!1,composed:!0})}};var ua=O`
  :host {
    --width: 31rem;
    --spacing: var(--wa-space-l);
    --backdrop-filter: none;
    --show-duration: var(--wa-transition-normal);
    --hide-duration: var(--wa-transition-normal);

    display: none;
  }

  :host([open]) {
    display: block;
  }

  .dialog {
    display: flex;
    flex-direction: column;
    top: 0;
    right: 0;
    bottom: 0;
    left: 0;
    width: var(--width);
    max-width: calc(100% - var(--wa-space-2xl));
    max-height: calc(100% - var(--wa-space-2xl));
    color: inherit;
    background-color: var(--wa-color-surface-raised);
    border-radius: var(--wa-panel-border-radius);
    border: none;
    box-shadow: var(--wa-shadow-l);
    padding: 0;
    margin: auto;

    &.show {
      animation: show-dialog var(--show-duration) ease;

      &::backdrop {
        animation: show-backdrop var(--show-duration, 200ms) ease;
      }
    }

    &.hide {
      animation: show-dialog var(--hide-duration) ease reverse;

      &::backdrop {
        animation: show-backdrop var(--hide-duration, 200ms) ease reverse;
      }
    }

    &.pulse {
      animation: pulse 250ms ease;
    }
  }

  .dialog:focus {
    outline: none;
  }

  /* Ensure there's enough vertical padding for phones that don't update vh when chrome appears (e.g. iPhone) */
  @media screen and (max-width: 420px) {
    .dialog {
      max-height: 80vh;
    }
  }

  .open {
    display: flex;
    opacity: 1;
  }

  .header {
    flex: 0 0 auto;
    display: flex;
    flex-wrap: nowrap;

    padding-inline-start: var(--spacing);
    padding-block-end: 0;

    /* Subtract the close button's padding so that the X is visually aligned with the edges of the dialog content */
    padding-inline-end: calc(var(--spacing) - var(--wa-form-control-padding-block));
    padding-block-start: calc(var(--spacing) - var(--wa-form-control-padding-block));
  }

  .title {
    align-self: center;
    flex: 1 1 auto;
    font-family: inherit;
    font-size: var(--wa-font-size-l);
    font-weight: var(--wa-font-weight-heading);
    line-height: var(--wa-line-height-condensed);
    margin: 0;
  }

  .header-actions {
    align-self: start;
    display: flex;
    flex-shrink: 0;
    flex-wrap: wrap;
    justify-content: end;
    gap: var(--wa-space-2xs);
    padding-inline-start: var(--spacing);
  }

  .header-actions wa-button,
  .header-actions ::slotted(wa-button) {
    flex: 0 0 auto;
    display: flex;
    align-items: center;
  }

  .body {
    flex: 1 1 auto;
    display: block;
    padding: var(--spacing);
    overflow: auto;
    -webkit-overflow-scrolling: touch;

    &:focus {
      outline: none;
    }

    &:focus-visible {
      outline: var(--wa-focus-ring);
      outline-offset: var(--wa-focus-ring-offset);
    }
  }

  .footer {
    flex: 0 0 auto;
    display: flex;
    flex-wrap: wrap;
    gap: var(--wa-space-xs);
    justify-content: end;
    padding: var(--spacing);
    padding-block-start: 0;
  }

  .footer ::slotted(wa-button:not(:first-of-type)) {
    margin-inline-start: var(--wa-spacing-xs);
  }

  .dialog::backdrop {
    /*
      NOTE: the ::backdrop element doesn't inherit properly in Safari yet, but it will in 17.4! At that time, we can
      remove the fallback values here.
    */
    background-color: var(--wa-color-overlay-modal, rgb(0 0 0 / 0.25));
    backdrop-filter: var(--backdrop-filter);
  }

  @keyframes pulse {
    0% {
      scale: 1;
    }
    50% {
      scale: 1.02;
    }
    100% {
      scale: 1;
    }
  }

  @keyframes show-dialog {
    from {
      opacity: 0;
      scale: 0.8;
    }
    to {
      opacity: 1;
      scale: 1;
    }
  }

  @keyframes show-backdrop {
    from {
      opacity: 0;
    }
    to {
      opacity: 1;
    }
  }

  @media (forced-colors: active) {
    .dialog {
      border: solid 1px white;
    }
  }
`;var X=[];function ha(t){X.push(t)}function fa(t){for(let e=X.length-1;e>=0;e--)if(X[e]===t){X.splice(e,1);break}}function vn(t){return X.length>0&&X[X.length-1]===t}function At(t,e){return new Promise(n=>{const i=new AbortController,{signal:o}=i;if(t.classList.contains(e))return;t.classList.add(e);let r=!1,a=()=>{r||(r=!0,t.classList.remove(e),n(),i.abort())};t.addEventListener("animationend",a,{once:!0,signal:o}),t.addEventListener("animationcancel",a,{once:!0,signal:o}),requestAnimationFrame(()=>{!r&&t.getAnimations().length===0&&a()})})}var F=class extends G{constructor(){super(...arguments),this.localize=new Ht(this),this.hasSlotController=new Fe(this,"footer","header-actions","label"),this.renderedWatcher=new ia(this,t=>this.handleRenderedChange(t)),this.open=!1,this.label="",this.withoutHeader=!1,this.lightDismiss=!1,this.withFooter=!1,this.handleDocumentKeyDown=t=>{t.key==="Escape"&&this.open&&vn(this)&&(t.preventDefault(),t.stopPropagation(),this.requestClose(this.dialog))}}firstUpdated(){this.open&&(this.addOpenListeners(),this.dialog.showModal(),te(this),this.renderedWatcher.start(this.dialog))}disconnectedCallback(){super.disconnectedCallback(),this.renderedWatcher.stop(),ee(this),this.removeOpenListeners()}async requestClose(t){const e=new la({source:t});if(this.dispatchEvent(e),e.defaultPrevented){this.open=!0,At(this.dialog,"pulse");return}this.removeOpenListeners(),await At(this.dialog,"hide"),this.open=!1,this.dialog.close(),ee(this),this.renderedWatcher.stop();const n=this.originalTrigger;typeof n?.focus=="function"&&setTimeout(()=>n.focus()),this.dispatchEvent(new da)}addOpenListeners(){document.addEventListener("keydown",this.handleDocumentKeyDown),ha(this)}removeOpenListeners(){document.removeEventListener("keydown",this.handleDocumentKeyDown),fa(this)}handleDialogCancel(t){t.preventDefault(),!this.dialog.classList.contains("hide")&&t.target===this.dialog&&vn(this)&&this.requestClose(this.dialog)}handleDialogClick(t){const n=t.target.closest('[data-dialog="close"]');n&&(t.stopPropagation(),this.requestClose(n))}async handleDialogPointerDown(t){t.target===this.dialog&&(this.lightDismiss?this.requestClose(this.dialog):await At(this.dialog,"pulse"))}handleRenderedChange(t){if(!this.open){this.renderedWatcher.stop();return}!t&&this.dialog.open?(this.removeOpenListeners(),this.dialog.close(),ee(this)):t&&!this.dialog.open&&(this.addOpenListeners(),this.dialog.showModal(),te(this))}handleOpenChange(){this.open&&!this.dialog.open?this.show():!this.open&&this.dialog.open?(this.open=!0,this.requestClose(this.dialog)):this.open||this.renderedWatcher.stop()}async show(){const t=new sa;if(this.dispatchEvent(t),t.defaultPrevented){this.open=!1;return}this.addOpenListeners(),this.originalTrigger=document.activeElement,this.open=!0,this.dialog.showModal(),te(this),this.renderedWatcher.start(this.dialog),requestAnimationFrame(()=>{const e=this.querySelector("[autofocus]");e&&typeof e.focus=="function"?e.focus():this.dialog.focus()}),await At(this.dialog,"show"),this.dispatchEvent(new ca)}render(){const t=!this.withoutHeader,e=this.hasSlotController.test("footer","withFooter");return z`
      <dialog
        part="dialog"
        class=${Bt({dialog:!0,open:this.open})}
        @cancel=${this.handleDialogCancel}
        @click=${this.handleDialogClick}
        @pointerdown=${this.handleDialogPointerDown}
      >
        ${t?z`
              <header part="header" class="header">
                <h2 part="title" class="title" id="title">
                  <!-- If there's no label, use an invisible character to prevent the header from collapsing -->
                  <slot name="label"> ${this.label.length>0?this.label:"​"} </slot>
                </h2>
                <div part="header-actions" class="header-actions">
                  <slot name="header-actions"></slot>
                  <wa-button
                    part="close-button"
                    exportparts="base:close-button__base"
                    class="close"
                    appearance="plain"
                    @click="${n=>this.requestClose(n.target)}"
                  >
                    <wa-icon
                      name="xmark"
                      label=${this.localize.term("close")}
                      library="system"
                      variant="solid"
                    ></wa-icon>
                  </wa-button>
                </div>
              </header>
            `:""}

        <div part="body" class="body"><slot></slot></div>

        <!-- Use a hidden element so we still get "slotchange" events. -->
        <footer part="footer" class="footer" ?hidden=${!e}>
          <slot name="footer"></slot>
        </footer>
      </dialog>
    `}};F.css=ua;d([Wt(".dialog")],F.prototype,"dialog",2);d([h({type:Boolean,reflect:!0})],F.prototype,"open",2);d([h({reflect:!0})],F.prototype,"label",2);d([h({attribute:"without-header",type:Boolean,reflect:!0})],F.prototype,"withoutHeader",2);d([h({attribute:"light-dismiss",type:Boolean})],F.prototype,"lightDismiss",2);d([h({attribute:"with-footer",type:Boolean})],F.prototype,"withFooter",2);d([D("open",{waitUntilFirstUpdate:!0})],F.prototype,"handleOpenChange",1);F=d([Lt("wa-dialog")],F);document.addEventListener("click",t=>{const e=t.target.closest("[data-dialog]");if(e instanceof Element){const[n,i]=aa(e.getAttribute("data-dialog")||"");if(n==="open"&&i?.length){const r=e.getRootNode().getElementById(i);r?.localName==="wa-dialog"?r.open=!0:console.warn(`A dialog with an ID of "${i}" could not be found in this document.`)}}}),document.addEventListener("pointerdown",()=>{});var ma=class extends Event{constructor(){super("wa-clear",{bubbles:!0,cancelable:!1,composed:!0})}};function pa(t,e){const n=t.metaKey||t.ctrlKey||t.shiftKey||t.altKey;t.key==="Enter"&&!n&&setTimeout(()=>{!t.defaultPrevented&&!t.isComposing&&ga(e)})}function ga(t){let e=null;if("form"in t&&(e=t.form),!e&&"getForm"in t&&(e=t.getForm()),!e)return;const n=[...e.elements];if(n.length===1){e.requestSubmit(null);return}const i=n.find(o=>o.type==="submit"&&!o.matches(":disabled"));i&&(["input","button"].includes(i.localName)?e.requestSubmit(i):i.click())}var va=O`
  :host {
    border-width: 0;
  }

  :host(:focus) {
    outline: none;
  }

  .text-field {
    display: flex;
    align-items: stretch;
    justify-content: start;
    position: relative;
    transition: inherit;
    height: var(--wa-form-control-height);
    border-color: var(--wa-form-control-border-color);
    border-radius: var(--wa-form-control-border-radius);
    border-style: var(--wa-form-control-border-style);
    border-width: var(--wa-form-control-border-width);
    cursor: text;
    color: var(--wa-form-control-value-color);
    font-size: var(--wa-form-control-value-font-size);
    font-family: inherit;
    font-weight: var(--wa-form-control-value-font-weight);
    line-height: var(--wa-form-control-value-line-height);
    vertical-align: middle;
    width: 100%;
    transition:
      background-color var(--wa-transition-normal),
      border-color var(--wa-transition-normal),
      outline-color var(--wa-transition-fast);
    transition-timing-function: var(--wa-transition-easing);
    background-color: var(--wa-form-control-background-color);
    box-shadow: var(--box-shadow);
    padding: 0 var(--wa-form-control-padding-inline);
    outline: var(--wa-focus-ring-style) var(--wa-focus-ring-width) transparent;
    outline-offset: var(--wa-focus-ring-offset);

    &:focus-within {
      outline-color: var(--wa-color-focus);
    }

    /* Style disabled inputs */
    &:has(:disabled) {
      cursor: not-allowed;
      opacity: 0.5;
    }
  }

  /* Appearance modifiers */
  :host([appearance='outlined']) .text-field {
    background-color: var(--wa-form-control-background-color);
    border-color: var(--wa-form-control-border-color);
  }

  :host([appearance='filled']) .text-field {
    background-color: var(--wa-color-neutral-fill-quiet);
    border-color: var(--wa-color-neutral-fill-quiet);
  }

  :host([appearance='filled-outlined']) .text-field {
    background-color: var(--wa-color-neutral-fill-quiet);
    border-color: var(--wa-form-control-border-color);
  }

  :host([pill]) .text-field {
    border-radius: var(--wa-border-radius-pill) !important;
  }

  .text-field {
    /* Show autofill styles over the entire text field, not just the native <input> */
    &:has(:autofill),
    &:has(:-webkit-autofill) {
      background-color: var(--wa-color-brand-fill-quiet) !important;
    }

    input,
    textarea {
      /*
      Fixes an alignment issue with placeholders.
      https://github.com/shoelace-style/webawesome/issues/342
    */
      height: 100%;

      padding: 0;
      border: none;
      outline: none;
      box-shadow: none;
      margin: 0;
      cursor: inherit;
      -webkit-appearance: none;
      font: inherit;

      /* Turn off Safari's autofill styles */
      &:-webkit-autofill,
      &:-webkit-autofill:hover,
      &:-webkit-autofill:focus,
      &:-webkit-autofill:active {
        -webkit-background-clip: text;
        background-color: transparent;
        -webkit-text-fill-color: inherit;
      }
    }
  }

  input {
    flex: 1 1 auto;
    min-width: 0;
    height: 100%;
    transition: inherit;

    /* prettier-ignore */
    background-color: rgb(118 118 118 / 0); /* ensures proper placeholder styles in webkit's date input */
    height: calc(var(--wa-form-control-height) - var(--border-width) * 2);
    padding-block: 0;
    color: inherit;

    &:autofill {
      &,
      &:hover,
      &:focus,
      &:active {
        box-shadow: none;
        caret-color: var(--wa-form-control-value-color);
      }
    }

    &::placeholder {
      color: var(--wa-form-control-placeholder-color);
      user-select: none;
      -webkit-user-select: none;
    }

    &::-webkit-search-decoration,
    &::-webkit-search-cancel-button,
    &::-webkit-search-results-button,
    &::-webkit-search-results-decoration {
      -webkit-appearance: none;
    }

    &:focus {
      outline: none;
    }
  }

  textarea {
    &:autofill {
      &,
      &:hover,
      &:focus,
      &:active {
        box-shadow: none;
        caret-color: var(--wa-form-control-value-color);
      }
    }

    &::placeholder {
      color: var(--wa-form-control-placeholder-color);
      user-select: none;
      -webkit-user-select: none;
    }
  }

  .start,
  .end {
    display: inline-flex;
    flex: 0 0 auto;
    align-items: center;
    cursor: default;

    &::slotted(wa-icon) {
      color: var(--wa-color-neutral-on-quiet);
    }
  }

  .start::slotted(*) {
    margin-inline-end: var(--wa-form-control-padding-inline);
  }

  .end::slotted(*) {
    margin-inline-start: var(--wa-form-control-padding-inline);
  }

  /*
   * Clearable + Password Toggle
   */

  .clear,
  .password-toggle {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    font-size: inherit;
    color: var(--wa-color-neutral-on-quiet);
    border: none;
    background: none;
    padding: 0;
    transition: var(--wa-transition-normal) color;
    cursor: pointer;
    margin-inline-start: var(--wa-form-control-padding-inline);

    @media (hover: hover) {
      &:hover {
        color: color-mix(in oklab, currentColor, var(--wa-color-mix-hover));
      }
    }

    &:active {
      color: color-mix(in oklab, currentColor, var(--wa-color-mix-active));
    }

    &:focus {
      outline: none;
    }
  }

  /* Don't show the browser's password toggle in Edge */
  ::-ms-reveal {
    display: none;
  }

  /* Hide the built-in number spinner */
  :host([without-spin-buttons]) input[type='number'] {
    -moz-appearance: textfield;

    &::-webkit-outer-spin-button,
    &::-webkit-inner-spin-button {
      -webkit-appearance: none;
      display: none;
    }
  }
`;var wa=O`
  :host {
    display: flex;
    flex-direction: column;
  }

  /* Treat wrapped labels, inputs, and hints as direct children of the host element */
  [part~='form-control'] {
    display: contents;
  }

  /* Label */
  :is([part~='form-control-label'], [part~='label']):has(*:not(:empty)),
  :is([part~='form-control-label'], [part~='label']).has-label {
    display: inline-flex;
    color: var(--wa-form-control-label-color);
    font-weight: var(--wa-form-control-label-font-weight);
    line-height: var(--wa-form-control-label-line-height);
    margin-block-end: 0.5em;
  }

  :host([required]) :is([part~='form-control-label'], [part~='label'])::after {
    content: var(--wa-form-control-required-content);
    margin-inline-start: var(--wa-form-control-required-content-offset);
    color: var(--wa-form-control-required-content-color);
  }

  /* Help text */
  [part~='hint'] {
    display: block;
    color: var(--wa-form-control-hint-color);
    font-weight: var(--wa-form-control-hint-font-weight);
    line-height: var(--wa-form-control-hint-line-height);
    margin-block-start: 0.5em;
    font-size: var(--wa-font-size-smaller);

    &:not(.has-slotted, .has-hint) {
      display: none;
    }
  }
`;const ba=ci(class extends di{constructor(t){if(super(t),t.type!==V.PROPERTY&&t.type!==V.ATTRIBUTE&&t.type!==V.BOOLEAN_ATTRIBUTE)throw Error("The `live` directive is not allowed on child or event bindings");if(!ta(t))throw Error("`live` bindings can only contain a single expression")}render(t){return t}update(t,[e]){if(e===E||e===C)return e;const n=t.element,i=t.name;if(t.type===V.PROPERTY){if(e===n[i])return E}else if(t.type===V.BOOLEAN_ATTRIBUTE){if(!!e===n.hasAttribute(i))return E}else if(t.type===V.ATTRIBUTE&&n.getAttribute(i)===e+"")return E;return na(t),e}});var p=class extends _{constructor(){super(...arguments),this.assumeInteractionOn=["blur","input"],this.hasSlotController=new Fe(this,"hint","label"),this.localize=new Ht(this),this.title="",this.type="text",this._value=null,this.defaultValue=this.getAttribute("value")||null,this.size="m",this.appearance="outlined",this.pill=!1,this.label="",this.hint="",this.withClear=!1,this.placeholder="",this.readonly=!1,this.passwordToggle=!1,this.passwordVisible=!1,this.withoutSpinButtons=!1,this.required=!1,this.spellcheck=!0,this.withLabel=!1,this.withHint=!1}static get validators(){return[...super.validators,Kn()]}get value(){return this.valueHasChanged?this._value:this._value??this.defaultValue}set value(t){this._value!==t&&(this.valueHasChanged=!0,this._value=t)}updateFormValue(t){if(t==null){this.setValue("",null);return}super.updateFormValue(t)}handleSizeChange(){ii(this.localName,this.size)}handleChange(t){this.value=this.input.value,this.relayNativeEvent(t,{bubbles:!0,composed:!0})}handleClearClick(t){t.preventDefault(),this.value!==""&&(this.value="",this.updateComplete.then(()=>{this.dispatchEvent(new ma),this.dispatchEvent(new InputEvent("input",{bubbles:!0,composed:!0})),this.dispatchEvent(new Event("change",{bubbles:!0,composed:!0}))})),this.input.focus()}handleInput(){this.value=this.input.value}handleKeyDown(t){pa(t,this)}handlePasswordToggle(){this.passwordVisible=!this.passwordVisible}updated(t){if(super.updated(t),t.has("value")||t.has("defaultValue")||t.has("type")){const e=["number","date","time","datetime-local"];this.input&&e.includes(this.type)&&this.value&&this.input.value!==this.value&&(this._value=this.input.value),this.customStates.set("blank",!this.value),this.updateValidity()}}handleStepChange(){this.input.step=String(this.step),this.updateValidity()}focus(t){this.input.focus(t)}blur(){this.input.blur()}select(){this.input.select()}setSelectionRange(t,e,n="none"){this.input.setSelectionRange(t,e,n)}setRangeText(t,e,n,i="preserve"){const o=e??this.input.selectionStart,r=n??this.input.selectionEnd;this.input.setRangeText(t,o,r,i),this.value!==this.input.value&&(this.value=this.input.value)}showPicker(){"showPicker"in HTMLInputElement.prototype&&this.input.showPicker()}stepUp(){this.input.stepUp(),this.value!==this.input.value&&(this.value=this.input.value)}stepDown(){this.input.stepDown(),this.value!==this.input.value&&(this.value=this.input.value)}formResetCallback(){this.value=null,this.input&&(this.input.value=this.value),super.formResetCallback()}render(){const t=this.hasSlotController.test("label","withLabel"),e=this.hasSlotController.test("hint","withHint"),n=this.label?!0:!!t,i=this.hint?!0:!!e,o=this.withClear&&!this.disabled&&!this.readonly,r=(!this.didSSR||this.hasUpdated)&&o&&(typeof this.value=="number"||this.value&&this.value.length>0);return z`
      <label
        part="form-control-label label"
        class=${Bt({label:!0,"has-label":n})}
        for="input"
        aria-hidden=${n?"false":"true"}
      >
        <slot name="label">${this.label}</slot>
      </label>

      <div part="base input-wrapper" class="text-field">
        <slot name="start" part="start" class="start"></slot>

        <input
          part="input"
          id="input"
          class="control"
          type=${this.type==="password"&&this.passwordVisible?"text":this.type}
          title=${this.title}
          name=${y(this.name)}
          ?disabled=${this.disabled}
          ?readonly=${this.readonly}
          ?required=${this.required}
          placeholder=${y(this.placeholder)}
          minlength=${y(this.minlength)}
          maxlength=${y(this.maxlength)}
          min=${y(this.min)}
          max=${y(this.max)}
          step=${y(this.step)}
          .value=${ba(this.value??"")}
          autocapitalize=${y(this.autocapitalize)}
          autocomplete=${y(this.autocomplete)}
          autocorrect=${this.autocorrect?"on":"off"}
          ?autofocus=${this.autofocus}
          spellcheck=${this.spellcheck}
          pattern=${y(this.pattern)}
          enterkeyhint=${y(this.enterkeyhint)}
          inputmode=${y(this.inputmode)}
          aria-describedby="hint"
          @change=${this.handleChange}
          @input=${this.handleInput}
          @keydown=${this.handleKeyDown}
        />

        ${r?z`
              <button
                part="clear-button"
                class="clear"
                type="button"
                aria-label=${this.localize.term("clearEntry")}
                @click=${this.handleClearClick}
                tabindex="-1"
              >
                <slot name="clear-icon">
                  <wa-icon name="circle-xmark" library="system" variant="regular"></wa-icon>
                </slot>
              </button>
            `:""}
        ${this.passwordToggle&&!this.disabled?z`
              <button
                part="password-toggle-button"
                class="password-toggle"
                type="button"
                aria-label=${this.localize.term(this.passwordVisible?"hidePassword":"showPassword")}
                @click=${this.handlePasswordToggle}
                tabindex="-1"
              >
                ${this.passwordVisible?z`
                      <slot name="hide-password-icon">
                        <wa-icon name="eye-slash" library="system" variant="regular"></wa-icon>
                      </slot>
                    `:z`
                      <slot name="show-password-icon">
                        <wa-icon name="eye" library="system" variant="regular"></wa-icon>
                      </slot>
                    `}
              </button>
            `:""}

        <slot name="end" part="end" class="end"></slot>
      </div>

      <slot
        id="hint"
        part="hint"
        name="hint"
        class=${Bt({"has-slotted":i})}
        aria-hidden=${i?"false":"true"}
        >${this.hint}</slot
      >
    `}};p.css=[oi,wa,va];p.shadowRootOptions={..._.shadowRootOptions,delegatesFocus:!0};d([Wt("input")],p.prototype,"input",2);d([h()],p.prototype,"title",2);d([h({reflect:!0})],p.prototype,"type",2);d([jt()],p.prototype,"value",1);d([h({attribute:"value",reflect:!0})],p.prototype,"defaultValue",2);d([h({reflect:!0})],p.prototype,"size",2);d([D("size")],p.prototype,"handleSizeChange",1);d([h({reflect:!0})],p.prototype,"appearance",2);d([h({type:Boolean,reflect:!0})],p.prototype,"pill",2);d([h()],p.prototype,"label",2);d([h({attribute:"hint"})],p.prototype,"hint",2);d([h({attribute:"with-clear",type:Boolean})],p.prototype,"withClear",2);d([h()],p.prototype,"placeholder",2);d([h({type:Boolean,reflect:!0})],p.prototype,"readonly",2);d([h({attribute:"password-toggle",type:Boolean})],p.prototype,"passwordToggle",2);d([h({attribute:"password-visible",type:Boolean})],p.prototype,"passwordVisible",2);d([h({attribute:"without-spin-buttons",type:Boolean,reflect:!0})],p.prototype,"withoutSpinButtons",2);d([h({type:Boolean,reflect:!0})],p.prototype,"required",2);d([h()],p.prototype,"pattern",2);d([h({type:Number})],p.prototype,"minlength",2);d([h({type:Number})],p.prototype,"maxlength",2);d([h()],p.prototype,"min",2);d([h()],p.prototype,"max",2);d([h()],p.prototype,"step",2);d([h()],p.prototype,"autocapitalize",2);d([h({type:Boolean,converter:{fromAttribute:t=>!(!t||t==="off"),toAttribute:t=>t?"on":"off"}})],p.prototype,"autocorrect",2);d([h()],p.prototype,"autocomplete",2);d([h({type:Boolean})],p.prototype,"autofocus",2);d([h()],p.prototype,"enterkeyhint",2);d([h({type:Boolean,converter:{fromAttribute:t=>!(!t||t==="false"),toAttribute:t=>t?"true":"false"}})],p.prototype,"spellcheck",2);d([h()],p.prototype,"inputmode",2);d([h({attribute:"with-label",type:Boolean})],p.prototype,"withLabel",2);d([h({attribute:"with-hint",type:Boolean})],p.prototype,"withHint",2);d([D("step",{waitUntilFirstUpdate:!0})],p.prototype,"handleStepChange",1);p=d([Lt("wa-input")],p);p.disableWarning?.("change-in-update");const Ne=document.querySelector("#app"),le=fe("initial"),fi=fe(0),ce=fe(!1),de=[];function ya(t){de.push(t),document.querySelector("[data-events]").textContent=de.join(",")}No(Ne,()=>{const t=fi.value;return I("section",{class:ce.value?"wa-dark app":"app","data-revision":t,children:[I("wa-input",{"data-testid":"name",label:"Name",value:le,hint:`revision ${t}`}),I("output",{"data-value":!0,children:le}),I("output",{"data-events":!0}),I("wa-button",{"data-action":"rerender",children:"Morph"}),I("wa-button",{"data-action":"theme",children:"Theme"}),I("wa-button",{"data-action":"open",variant:"brand",children:"Open dialog"}),I("wa-dialog",{"data-testid":"dialog",label:"Confirm",children:["Dialog body",I("wa-button",{slot:"footer","data-action":"close",children:"Close"})]})]})});for(const t of["input","change","wa-input","wa-change","wa-show","wa-hide","wa-after-show","wa-after-hide"])Xn(Ne,t,"wa-input, wa-dialog",(e,n)=>{ya(e.type),n.matches("wa-input")&&(e.type==="input"||e.type==="wa-input")&&(le.value=n.value)});Xn(Ne,"click","[data-action]",(t,e)=>{switch(e.dataset.action){case"rerender":fi.value+=1;break;case"theme":ce.value=!ce.value;break;case"open":document.querySelector('[data-testid="dialog"]').show();break;case"close":document.querySelector('[data-testid="dialog"]').hide();break}});Object.assign(globalThis,{spike:{events:de,input:()=>document.querySelector('[data-testid="name"]'),dialog:()=>document.querySelector('[data-testid="dialog"]')}});
