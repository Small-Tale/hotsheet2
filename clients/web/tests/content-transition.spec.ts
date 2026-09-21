import {expect,test} from '@playwright/test';

const pausedMotion='.content-transition__side { animation-play-state: paused !important; }';

test('demonstrates adjacent full-width A-B push/pop with chrome crossfades and reduced motion',async({page})=>{
  await page.goto('/ux-demo?component=content-transition');
  const transition=page.locator('[data-component="content-transition"][data-transition-region="content"]'),label=page.locator('[data-transition-region="label"]'),footer=page.locator('[data-transition-region="footer"]');
  await expect(transition).toHaveAttribute('data-active-side','a');

  // Freeze before activation so a loaded parallel runner cannot let the 280 ms CSS motion finish
  // before the midpoint geometry assertion samples it.
  const forwardPause=await page.addStyleTag({content:pausedMotion});
  await page.getByRole('button',{name:'Continue',exact:true}).click();
  await expect(transition).toHaveAttribute('data-active-side','b');
  await expect.poll(()=>transition.locator('[data-side="a"]').evaluate(node=>getComputedStyle(node).animationName)).toBe('content-transition-push-out-start');
  await expect.poll(()=>transition.locator('[data-side="b"]').evaluate(node=>getComputedStyle(node).animationName)).toBe('content-transition-push-in-end');
  await expect.poll(()=>label.locator('[data-side="b"]').evaluate(node=>getComputedStyle(node).animationName)).toBe('content-transition-fade-in');
  await expect.poll(()=>footer.locator('[data-side="b"]').evaluate(node=>getComputedStyle(node).animationName)).toBe('content-transition-fade-in');
  const geometry=await transition.evaluate(node=>{for(const animation of node.getAnimations({subtree:true}))animation.currentTime=140;const[a,b]=[...node.querySelectorAll<HTMLElement>(':scope > [data-side]')].map(side=>side.getBoundingClientRect());const frame=node.getBoundingClientRect();return{frameWidth:frame.width,aWidth:a.width,bWidth:b.width,seam:b.left-a.right}});
  expect(geometry.aWidth).toBeCloseTo(geometry.frameWidth,0);expect(geometry.bWidth).toBeCloseTo(geometry.frameWidth,0);expect(Math.abs(geometry.seam)).toBeLessThanOrEqual(1);
  await forwardPause.evaluate(node=>{node.parentNode?.removeChild(node)});
  await expect(transition.locator('[data-side="a"]')).toHaveCSS('opacity','0');
  await page.screenshot({path:'/private/tmp/hs2-0epnb7-content-transition-push-after.png',fullPage:true});

  await page.setViewportSize({width:720,height:720});
  await transition.scrollIntoViewIfNeeded();
  await expect(transition).toBeInViewport();
  await expect.poll(()=>page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth)).toBe(true);
  await page.screenshot({path:'/private/tmp/hs2-0epnb7-content-transition-push-after-narrow.png',fullPage:true});

  const backwardPause=await page.addStyleTag({content:pausedMotion});
  await page.getByRole('button',{name:'Back',exact:true}).click();
  await expect.poll(()=>transition.locator('[data-side="a"]').evaluate(node=>getComputedStyle(node).animationName)).toBe('content-transition-push-in-start');
  await backwardPause.evaluate(node=>{node.parentNode?.removeChild(node)});

  await page.getByRole('button',{name:'Settings',exact:true}).click();
  const crossfadePause=await page.addStyleTag({content:pausedMotion});
  for(const[name,value]of[['transition-style','crossfade'],['transition-side','b']]as const)await page.locator(`wa-select[name="${name}"]`).evaluate((node:HTMLElement&{value:string},next)=>{node.value=next;node.dispatchEvent(new Event('change',{bubbles:true}))},value);
  await expect(transition).toHaveAttribute('data-transition-style','crossfade');
  await expect.poll(()=>transition.locator('[data-side="b"]').evaluate(node=>getComputedStyle(node).animationName)).toBe('content-transition-fade-in');
  await page.emulateMedia({reducedMotion:'reduce'});
  expect(parseFloat(await transition.locator('[data-side="b"]').evaluate(node=>getComputedStyle(node).animationDuration))).toBeLessThanOrEqual(.001);
  await crossfadePause.evaluate(node=>{node.parentNode?.removeChild(node)});
});
