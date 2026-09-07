import {expect,test} from '@playwright/test';

const installFixture=async(page:import('@playwright/test').Page)=>{
  await page.goto('/ux-demo');
  await page.evaluate(async()=>{
    const modulePath='/src/ticket-motion.ts';
    (window as typeof window&{ticketMotion?:typeof import('../src/ticket-motion')}).ticketMotion=await import(modulePath) as typeof import('../src/ticket-motion');
    document.body.innerHTML=`
      <style>
        body { margin: 0; }
        .app-shell__workspace { position: fixed; inset: 40px auto auto 40px; width: 560px; }
        [data-component="ticket-board"] { display: grid; grid-template-columns: repeat(2, 240px); gap: 48px; }
        [data-column-id] { display: grid; align-content: start; gap: 8px; }
        .ticket-list-row-container { width: 240px; container-type: inline-size; }
        .ticket-list-row { display: block; width: 100%; height: 100%; box-sizing: border-box; border: 1px solid #789; border-radius: 10px; background: white; }
        .ticket-list-row__body { display: block; box-sizing: border-box; height: 100%; padding: 6px; }
        @container (max-width: 250px) { .ticket-list-row__body { padding: 18px; } }
      </style>
      <main class="app-shell__workspace" data-presentation="edge-to-edge">
        <section data-component="ticket-board">
          <div data-column-id="not-started"></div>
          <div data-column-id="started"></div>
        </section>
      </main>`;
  });
};

test('naturally phases an exact-height arrival without changing internal container layout',async({page})=>{
  await installFixture(page);
  const frames=await page.evaluate(async()=>{
    const motion=(window as typeof window&{ticketMotion:typeof import('../src/ticket-motion')}).ticketMotion,column=document.querySelector<HTMLElement>('[data-column-id="not-started"]')!;
    const makeRow=(slug:string,height:number)=>{const container=document.createElement('div');container.className='ticket-list-row-container';container.dataset.component='ticket-list-row-container';container.dataset.key=`ticket:${slug}`;container.style.height=`${height}px`;container.innerHTML=`<article class="ticket-list-row" data-component="ticket-list-row" data-ticket-slug="${slug}" role="option"><div class="ticket-list-row__body"><strong>${slug} stable content</strong></div></article>`;return container};
    const retained=makeRow('HS2-RETAINED',46);column.append(retained);const beforeTop=retained.getBoundingClientRect().top,snapshot=motion.captureTicketMotion(document);
    const incoming=makeRow('HS2-INCOMING',72);column.prepend(incoming);const finalTop=retained.getBoundingClientRect().top;motion.animateTicketMotion(snapshot,document,false);
    await new Promise(resolve=>{requestAnimationFrame(()=>{resolve(undefined)})});const earlyTop=retained.getBoundingClientRect().top,ghost=document.querySelector<HTMLElement>('[data-ticket-motion-ghost="incoming"]')!,realBody=incoming.querySelector<HTMLElement>('.ticket-list-row__body')!,ghostBody=ghost.querySelector<HTMLElement>('.ticket-list-row__body')!,padding=[getComputedStyle(realBody).paddingLeft,getComputedStyle(ghostBody).paddingLeft],widths=[incoming.getBoundingClientRect().width,ghost.getBoundingClientRect().width];
    await new Promise(resolve=>{setTimeout(resolve,110)});const midTop=retained.getBoundingClientRect().top,midOpacity=Number.parseFloat(getComputedStyle(ghost).opacity);
    await new Promise(resolve=>{setTimeout(resolve,190)});const fadeOpacity=Number.parseFloat(getComputedStyle(ghost).opacity),settledTop=retained.getBoundingClientRect().top;
    await new Promise(resolve=>{setTimeout(resolve,140)});return{beforeTop,finalTop,earlyTop,midTop,midOpacity,fadeOpacity,settledTop,padding,widths,ghostGone:!document.querySelector('[data-ticket-motion-ghost="incoming"]'),realVisibility:getComputedStyle(incoming).visibility};
  });
  expect(frames.finalTop-frames.beforeTop).toBeCloseTo(80,0);expect(Math.abs(frames.earlyTop-frames.beforeTop)).toBeLessThan(12);expect(frames.midTop).toBeGreaterThan(frames.beforeTop);expect(frames.midTop).toBeLessThan(frames.finalTop);expect(frames.midOpacity).toBe(0);expect(frames.fadeOpacity).toBeGreaterThan(0);expect(frames.fadeOpacity).toBeLessThan(1);expect(frames.settledTop).toBeCloseTo(frames.finalTop,0);expect(frames.padding).toEqual(['18px','18px']);expect(frames.widths[1]).toBeCloseTo(frames.widths[0],0);expect(frames.ghostGone).toBe(true);expect(frames.realVisibility).toBe('visible');
});

test('naturally fades a departure before collapsing its exact outer-wrapper space',async({page})=>{
  await installFixture(page);
  const frames=await page.evaluate(async()=>{
    const motion=(window as typeof window&{ticketMotion:typeof import('../src/ticket-motion')}).ticketMotion,column=document.querySelector<HTMLElement>('[data-column-id="not-started"]')!;
    const makeRow=(slug:string,height:number)=>{const container=document.createElement('div');container.className='ticket-list-row-container';container.dataset.component='ticket-list-row-container';container.style.height=`${height}px`;container.innerHTML=`<article class="ticket-list-row" data-component="ticket-list-row" data-ticket-slug="${slug}"><div class="ticket-list-row__body">${slug}</div></article>`;return container};
    const outgoing=makeRow('HS2-OUTGOING',67),retained=makeRow('HS2-RETAINED',49);column.append(outgoing,retained);const beforeTop=retained.getBoundingClientRect().top,snapshot=motion.captureTicketMotion(document);outgoing.remove();const finalTop=retained.getBoundingClientRect().top;motion.animateTicketMotion(snapshot,document,false);
    await new Promise(resolve=>{requestAnimationFrame(()=>{resolve(undefined)})});const earlyTop=retained.getBoundingClientRect().top,ghost=document.querySelector<HTMLElement>('[data-ticket-motion-ghost="outgoing"]')!;
    await new Promise(resolve=>{setTimeout(resolve,85)});const fadeTop=retained.getBoundingClientRect().top,fadeOpacity=Number.parseFloat(getComputedStyle(ghost).opacity);
    await new Promise(resolve=>{setTimeout(resolve,150)});const collapseTop=retained.getBoundingClientRect().top;
    await new Promise(resolve=>{setTimeout(resolve,190)});return{beforeTop,finalTop,earlyTop,fadeTop,fadeOpacity,collapseTop,settledTop:retained.getBoundingClientRect().top,ghostGone:!document.querySelector('[data-ticket-motion-ghost="outgoing"]')};
  });
  expect(frames.beforeTop-frames.finalTop).toBeCloseTo(75,0);expect(Math.abs(frames.earlyTop-frames.beforeTop)).toBeLessThan(3);expect(Math.abs(frames.fadeTop-frames.beforeTop)).toBeLessThan(3);expect(frames.fadeOpacity).toBeGreaterThan(0);expect(frames.fadeOpacity).toBeLessThan(1);expect(frames.collapseTop).toBeLessThan(frames.beforeTop);expect(frames.collapseTop).toBeGreaterThan(frames.finalTop);expect(frames.settledTop).toBeCloseTo(frames.finalTop,0);expect(frames.ghostGone).toBe(true);
});

test('naturally moves a container overlay while both columns reflow together',async({page})=>{
  await installFixture(page);
  const frames=await page.evaluate(async()=>{
    const motion=(window as typeof window&{ticketMotion:typeof import('../src/ticket-motion')}).ticketMotion,source=document.querySelector<HTMLElement>('[data-column-id="not-started"]')!,destination=document.querySelector<HTMLElement>('[data-column-id="started"]')!;
    const makeRow=(slug:string,height:number)=>{const container=document.createElement('div');container.className='ticket-list-row-container';container.dataset.component='ticket-list-row-container';container.style.height=`${height}px`;container.innerHTML=`<article class="ticket-list-row" data-component="ticket-list-row" data-ticket-slug="${slug}"><div class="ticket-list-row__body">${slug}</div></article>`;return container};
    const moving=makeRow('HS2-MOVING',64),sourceSibling=makeRow('HS2-SOURCE',48),destinationSibling=makeRow('HS2-DESTINATION',48);source.append(moving,sourceSibling);destination.append(destinationSibling);const sourceStart=sourceSibling.getBoundingClientRect().top,destinationStart=destinationSibling.getBoundingClientRect().top,snapshot=motion.captureTicketMotion(document);
    destination.prepend(moving);const sourceFinal=sourceSibling.getBoundingClientRect().top,destinationFinal=destinationSibling.getBoundingClientRect().top,movingFinal=moving.getBoundingClientRect();motion.animateTicketMotion(snapshot,document,false);
    await new Promise(resolve=>{requestAnimationFrame(()=>{resolve(undefined)})});const ghost=document.querySelector<HTMLElement>('[data-ticket-motion-ghost="move"]')!,ghostStart=ghost.getBoundingClientRect();
    await new Promise(resolve=>{setTimeout(resolve,110)});const ghostMid=ghost.getBoundingClientRect(),sourceMid=sourceSibling.getBoundingClientRect().top,destinationMid=destinationSibling.getBoundingClientRect().top;
    await new Promise(resolve=>{setTimeout(resolve,170)});return{sourceStart,sourceFinal,destinationStart,destinationFinal,movingFinal:{x:movingFinal.x,y:movingFinal.y},ghostStart:{x:ghostStart.x,y:ghostStart.y},ghostMid:{x:ghostMid.x,y:ghostMid.y},sourceMid,destinationMid,ghostGone:!document.querySelector('[data-ticket-motion-ghost="move"]'),realVisibility:getComputedStyle(moving).visibility};
  });
  expect(frames.ghostStart.x).toBeLessThan(frames.movingFinal.x);expect(frames.ghostMid.x).toBeGreaterThan(frames.ghostStart.x);expect(frames.ghostMid.x).toBeLessThan(frames.movingFinal.x);expect(frames.sourceMid).toBeLessThan(frames.sourceStart);expect(frames.sourceMid).toBeGreaterThan(frames.sourceFinal);expect(frames.destinationMid).toBeGreaterThan(frames.destinationStart);expect(frames.destinationMid).toBeLessThan(frames.destinationFinal);expect(frames.ghostGone).toBe(true);expect(frames.realVisibility).toBe('visible');
});

test('captures the container-stable cross-column overlay in flight',async({page})=>{
  await installFixture(page);
  await page.evaluate(()=>{
    const motion=(window as typeof window&{ticketMotion:typeof import('../src/ticket-motion')}).ticketMotion,source=document.querySelector<HTMLElement>('[data-column-id="not-started"]')!,destination=document.querySelector<HTMLElement>('[data-column-id="started"]')!;
    const makeRow=(slug:string)=>{const container=document.createElement('div');container.className='ticket-list-row-container';container.dataset.component='ticket-list-row-container';container.style.height='120px';container.innerHTML=`<article class="ticket-list-row" data-component="ticket-list-row" data-ticket-slug="${slug}"><div class="ticket-list-row__body"><strong>${slug}</strong><br>container-query content</div></article>`;return container};
    const moving=makeRow('HS2-MOVING'),sourceSibling=makeRow('HS2-SOURCE'),destinationSibling=makeRow('HS2-DESTINATION');source.append(moving,sourceSibling);destination.append(destinationSibling);const snapshot=motion.captureTicketMotion(document);destination.prepend(moving);motion.animateTicketMotion(snapshot,document,false);
  });
  await page.waitForTimeout(110);const ghost=page.locator('[data-ticket-motion-ghost="move"]');await expect(ghost).toBeAttached();await expect(ghost).not.toContainText('HS2-MOVING');await expect(page.locator('[data-component="ticket-list-row"][data-ticket-slug="HS2-MOVING"]')).toHaveCount(1);await page.screenshot({path:'/private/tmp/hs2-jgwtjj-ticket-motion-midflight-after.png',fullPage:true});
  await expect(page.locator('[data-ticket-motion-ghost="move"]')).toHaveCount(0);await expect(page.locator('[data-component="ticket-list-row"][data-ticket-slug="HS2-MOVING"]')).toBeVisible();
});
