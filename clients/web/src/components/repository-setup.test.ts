import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

import { describe,expect,it } from 'vitest';

import { RepositorySetup } from './repository-setup';

describe('RepositorySetup',()=>{
  it('offers a bounded initialization action without promising to commit files',()=>{
    const markup=String(RepositorySetup({}));
    expect(markup).toContain('data-step="initialize"');
    expect(markup).not.toContain('This folder is not a Git repository');
    expect(markup).toContain('data-action="initialize-repository"');
    expect(markup).toContain('Initialize Git repository');
    expect(markup).toContain('will not stage or commit');
  });

  it('offers provider-neutral origin setup without publishing project contents',()=>{
    const markup=String(RepositorySetup({step:'remote',error:'origin could not be added'}));
    expect(markup).toContain('data-action="connect-repository-remote"');
    expect(markup).toContain('name="repository-remote"');
    expect(markup).toContain('data-action="skip-repository-remote"');
    expect(markup).toContain('Add origin');
    expect(markup).toContain('will not stage, commit, or push');
    expect(markup).toContain('role="alert"');
    expect(markup).toContain('origin could not be added');
  });

  it('exposes deterministic busy copy and a narrow responsive layout',()=>{
    expect(String(RepositorySetup({busy:true}))).toContain('Initializing…');
    expect(String(RepositorySetup({step:'remote',busy:true}))).toContain('Adding origin…');
    const css=readFileSync(resolve(import.meta.dirname,'repository-setup.css'),'utf8');
    expect(css).toMatch(/@media \(max-width: 32rem\)/);
    expect(css).toMatch(/width: min\(30rem, 100%\)/);
    expect(css).toMatch(/data-step="initialize"[^}]*footer \{[^}]*align-items: center;[^}]*justify-content: center;/);
  });
});
