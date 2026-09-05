import {describe,expect,it} from 'vitest';

import {attachmentReferences,attachmentReferenceUrl,expandAttachmentReferences,isImageAttachment,parseAttachmentReference} from './attachment-references';

const context={baseUrl:'/project-api/demo',checkout:'checkout one',ticket:'HS2-LOCAL'};

describe('attachment references',()=>{
  it('parses local and cross-ticket names without losing spaces',()=>{
    expect(parseAttachmentReference('attachment:screen shot.png')).toEqual({filename:'screen shot.png'});
    expect(parseAttachmentReference('attachment:[HS2-OTHER]report.pdf')).toEqual({ticket:'HS2-OTHER',filename:'report.pdf'});
    expect(parseAttachmentReference('not-an-attachment')).toBeUndefined();
  });

  it('builds encoded checkout URLs and recognizes browser image formats',()=>{
    expect(attachmentReferenceUrl(context,{ticket:'HS2-OTHER',filename:'wide view.svg'})).toBe('/project-api/demo/checkouts/checkout%20one/tickets/HS2-OTHER/attachments/by-name/wide%20view.svg');
    expect(isImageAttachment('capture.SVG')).toBe(true);expect(isImageAttachment('notes.md')).toBe(false);
  });

  it('expands code and standard Markdown references while retaining a discoverable list',()=>{
    const source='See attachment:diagram.svg, `attachment:report.pdf`, `attachment:[HS2-OTHER]screen shot.png`, and [raw file](attachment:data.json).';
    const expanded=expandAttachmentReferences(source,context);
    expect(expanded).toContain('[report.pdf](/project-api/demo/checkouts/checkout%20one/tickets/HS2-LOCAL/attachments/by-name/report.pdf "attachment:report.pdf")');
    expect(expanded).toContain('![diagram.svg](/project-api/demo/checkouts/checkout%20one/tickets/HS2-LOCAL/attachments/by-name/diagram.svg');
    expect(expanded).toContain('![screen shot.png](/project-api/demo/checkouts/checkout%20one/tickets/HS2-OTHER/attachments/by-name/screen%20shot.png');
    expect(expanded).toContain('[raw file](/project-api/demo/checkouts/checkout%20one/tickets/HS2-LOCAL/attachments/by-name/data.json');
    expect(attachmentReferences(source)).toEqual([{filename:'diagram.svg'},{filename:'report.pdf'},{ticket:'HS2-OTHER',filename:'screen shot.png'},{filename:'data.json'}]);
  });
});
