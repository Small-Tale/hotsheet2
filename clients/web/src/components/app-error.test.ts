import { expect,it } from 'vitest';

import { AppError } from './app-error';

it('renders the error with an accessible dismissal action',()=>{
  const markup=String(AppError({message:'The operation failed.'}));
  expect(markup).toContain('data-component="app-error"');
  expect(markup).toContain('role="alert"');
  expect(markup).toContain('The operation failed.');
  expect(markup).toContain('data-action="dismiss-app-error"');
  expect(markup).toContain('aria-label="Dismiss error"');
  expect(markup).toContain('data-lucide="x"');
});
