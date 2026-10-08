/** Keep provider identity in the data model while shortening GitHub issue numbers in local UI. */
export function ticketDisplayNumber(slug: string, provider?: string): string {
  if (provider !== 'github') return slug;
  const issue = /^[^/#]+\/[^/#]+#([1-9]\d*)$/.exec(slug);
  return issue ? `#${issue[1]}` : slug;
}
