import { describe, expect, it, vi } from 'vitest';

import { NotWorkingSubmissionError, submitNotWorkingReport } from './not-working-workflow';

const file = (name: string) => new File([name], name, { type: 'text/plain' });

describe('submitNotWorkingReport', () => {
  it('accepts note-only and attachment-only reports', async () => {
    const report = vi.fn().mockResolvedValue(undefined);
    await submitNotWorkingReport({ note: ' regression ', files: [] }, { report });
    expect(report).toHaveBeenCalledWith('regression', []);
    const proof = file('proof.txt');
    await submitNotWorkingReport({ note: '', files: [proof] }, { report });
    expect(report).toHaveBeenLastCalledWith('', [proof]);
  });

  it('rejects an empty report before mutating anything', async () => {
    const workflow = { report: vi.fn() };
    await expect(submitNotWorkingReport({ note: ' ', files: [] }, workflow)).rejects.toThrow('Describe what is wrong');
    expect(workflow.report).not.toHaveBeenCalled();
  });

  it('surfaces an atomic provider failure without issuing compensating calls', async () => {
    const report = vi.fn().mockRejectedValue(new Error('atomic report failed'));
    const result = submitNotWorkingReport({ note: 'broken', files: [file('proof')] }, { report });
    await expect(result).rejects.toBeInstanceOf(NotWorkingSubmissionError);
    await expect(result).rejects.toThrow('atomic report failed');
    expect(report).toHaveBeenCalledTimes(1);
  });
});
