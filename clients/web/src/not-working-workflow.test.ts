import { describe, expect, it, vi } from 'vitest';

import { NotWorkingSubmissionError, submitNotWorkingReport } from './not-working-workflow';

const file = (name: string) => new File([name], name, { type: 'text/plain' });

describe('submitNotWorkingReport', () => {
  it('accepts note-only and attachment-only reports', async () => {
    const reopen = vi.fn().mockResolvedValue(undefined);
    await submitNotWorkingReport({ note: ' regression ', files: [] }, { upload: vi.fn(), removeAttachment: vi.fn(), reopen });
    expect(reopen).toHaveBeenCalledWith('regression');
    const upload = vi.fn().mockResolvedValue('A1');
    await submitNotWorkingReport({ note: '', files: [file('proof.txt')] }, { upload, removeAttachment: vi.fn(), reopen });
    expect(upload).toHaveBeenCalledTimes(1);
    expect(reopen).toHaveBeenLastCalledWith(undefined);
  });

  it('rejects an empty report before mutating anything', async () => {
    const workflow = { upload: vi.fn(), removeAttachment: vi.fn(), reopen: vi.fn() };
    await expect(submitNotWorkingReport({ note: ' ', files: [] }, workflow)).rejects.toThrow('Describe what is wrong');
    expect(workflow.reopen).not.toHaveBeenCalled();
  });

  it('compensates prior uploads in reverse order after an upload or reopen failure', async () => {
    const removeAttachment = vi.fn().mockResolvedValue(undefined);
    const upload = vi.fn().mockResolvedValueOnce('A1').mockResolvedValueOnce('A2');
    await expect(submitNotWorkingReport({ note: 'broken', files: [file('one'), file('two')] }, { upload, removeAttachment, reopen: vi.fn().mockRejectedValue(new Error('patch failed')) })).rejects.toThrow('patch failed');
    expect(removeAttachment.mock.calls).toEqual([['A2'], ['A1']]);

    removeAttachment.mockClear();
    upload.mockReset().mockResolvedValueOnce('A3').mockRejectedValueOnce(new Error('upload failed'));
    await expect(submitNotWorkingReport({ note: '', files: [file('one'), file('two')] }, { upload, removeAttachment, reopen: vi.fn() })).rejects.toThrow('upload failed');
    expect(removeAttachment).toHaveBeenCalledWith('A3');
  });

  it('reports cleanup failure instead of concealing partial evidence', async () => {
    const result = submitNotWorkingReport({ note: 'broken', files: [file('proof')] }, {
      upload: vi.fn().mockResolvedValue('A1'), removeAttachment: vi.fn().mockRejectedValue(new Error('delete failed')), reopen: vi.fn().mockRejectedValue(new Error('patch failed')),
    });
    await expect(result).rejects.toBeInstanceOf(NotWorkingSubmissionError);
    await expect(result).rejects.toThrow('Some uploaded evidence could not be removed');
  });
});
