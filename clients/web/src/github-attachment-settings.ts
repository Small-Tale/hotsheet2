/** Settings shared by the GitHub connection form and its assets-repository adapter (HS2-HSA64D). */
export const DEFAULT_GITHUB_ATTACHMENT_FOLDER = 'hotsheet-attachments';
export const DEFAULT_GITHUB_ATTACHMENT_BRANCH = 'main';

/** Match GitHubAttachmentRepository::new on the server before submitting a connection. */
export function githubAttachmentSettings(
  repositoryInput: string,
  folderInput: string,
  branchInput: string,
): { settings: Record<string, string>; error?: never } | { settings?: never; error: string } {
  const repository = repositoryInput.trim();
  if (!repository) return { settings: {} };
  const validName = (name: string) => Boolean(name) && name !== '.' && name !== '..' && /^[A-Za-z0-9._-]+$/.test(name);
  const parts = repository.split('/');
  if (parts.length !== 2 || !parts.every(validName))
    return { error: 'Enter an attachment repository as owner/repository.' };
  const folder = (folderInput.trim() || DEFAULT_GITHUB_ATTACHMENT_FOLDER).replace(/^\/+|\/+$/g, '');
  if (folder.split('/').some((part) => part === '.' || part === '..' || (!part && Boolean(folder))))
    return { error: 'Enter a plain relative attachment folder without empty, . or .. segments.' };
  const branch = branchInput.trim() || DEFAULT_GITHUB_ATTACHMENT_BRANCH;
  if (branch.includes('..') || /\s/.test(branch))
    return { error: 'Enter an attachment branch without whitespace or consecutive dots.' };
  return { settings: { attachment_repo: repository, attachment_folder: folder, attachment_branch: branch } };
}
