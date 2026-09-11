import { expect, test } from '@playwright/test';

const adversarialMarkdown = `## Safe GFM survives

- [x] Task item
- ~~Strikethrough~~

| Surface | Result |
| --- | --- |
| Markdown | Safe |

<script>window.__markdownXssAudit.script = true</script>
<img src="/markdown-xss-event.png" onerror="window.__markdownXssAudit.handler = true">
<svg onload="window.__markdownXssAudit.svg = true"></svg>

[Unsafe JavaScript link](javascript:window.__markdownXssAudit.link=true)
[Unsafe data link](data:text/html,%3Cscript%3Ewindow.__markdownXssAudit.dataLink%3Dtrue%3C%2Fscript%3E)
![Unsafe JavaScript image](javascript:window.__markdownXssAudit.image=true)
![Unsafe data image](data:image/svg+xml,%3Csvg%20xmlns%3D%22http%3A%2F%2Fwww.w3.org%2F2000%2Fsvg%22%20onload%3D%22window.__markdownXssAudit.dataImage%3Dtrue%22%3E%3C%2Fsvg%3E)

[Safe guide](/ux-demo?component=tag-chip "Component guide")

Image evidence: attachment:reader-wireframe.png.
Document evidence: attachment:reader-notes.md.
Related ticket: HS2-SAFE12.
`;

test('keeps attacker Markdown inert through kerf raw while preserving safe GFM and actions', async ({ page }) => {
  const dialogs: string[] = [];
  const suspiciousRequests: string[] = [];
  page.on('dialog', async dialog => {
    dialogs.push(dialog.message());
    await dialog.dismiss();
  });
  page.on('request', request => {
    if (/markdown-xss-event|^(?:javascript|data):/i.test(request.url())) suspiciousRequests.push(request.url());
  });
  await page.addInitScript(() => {
    Reflect.set(window, '__markdownXssAudit', {
      script: false,
      handler: false,
      svg: false,
      link: false,
      dataLink: false,
      image: false,
      dataImage: false,
    });
  });

  await page.goto('/ux-demo?component=ticket-reader');
  const note = page.locator('[data-component="note-card"][data-note-id="reader-note"]');
  await note.locator('.note-card__body').dblclick();
  const editor = note.getByRole('textbox', { name: 'Note body' });
  await editor.fill(adversarialMarkdown);
  await editor.blur();

  const preview = note.locator('[data-component="markdown-preview"]');
  await expect(preview.getByRole('heading', { name: 'Safe GFM survives' })).toBeVisible();
  await expect(preview.locator('input[type="checkbox"]')).toBeDisabled();
  await expect(preview.locator('del')).toHaveText('Strikethrough');
  await expect(preview.locator('table')).toContainText('Markdown');
  await expect(preview.locator('script, svg, [onerror], [onload], [onclick]')).toHaveCount(0);
  await expect(preview).toContainText('<script>window.__markdownXssAudit.script = true</script>');
  await expect(preview).toContainText('<img src="/markdown-xss-event.png" onerror="window.__markdownXssAudit.handler = true">');

  const unsafeLinks = preview.getByRole('link', { name: /^Unsafe/ });
  await expect(unsafeLinks).toHaveCount(2);
  for (const link of await unsafeLinks.all()) {
    await expect(link).toHaveAttribute('href', '#');
    await link.click();
  }
  const unsafeImages = preview.locator('img[alt^="Unsafe"]');
  await expect(unsafeImages).toHaveCount(2);
  for (const image of await unsafeImages.all()) await expect(image).toHaveAttribute('src', '#');

  const safeGuide = preview.getByRole('link', { name: 'Safe guide' });
  await expect(safeGuide).toHaveAttribute('href', '/ux-demo?component=tag-chip');
  await expect(safeGuide).toHaveAttribute('target', '_blank');
  await expect(safeGuide).toHaveAttribute('rel', 'noopener noreferrer');
  await expect(preview.locator('[data-action="open-attachment-gallery"]')).toHaveAttribute('data-attachment-name', 'reader-wireframe.png');
  await expect(preview.locator('[data-action="open-referenced-attachment"]')).toHaveAttribute('data-attachment-name', 'reader-notes.md');
  await expect(preview.locator('[data-action="open-linked-ticket"]')).toHaveAttribute('data-ticket-slug', 'HS2-SAFE12');

  await expect.poll(() => page.evaluate(() => Reflect.get(window, '__markdownXssAudit'))).toEqual({
    script: false,
    handler: false,
    svg: false,
    link: false,
    dataLink: false,
    image: false,
    dataImage: false,
  });
  expect(dialogs).toEqual([]);
  expect(suspiciousRequests).toEqual([]);
});
