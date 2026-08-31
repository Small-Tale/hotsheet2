import eslint from '@eslint/js';
import importX from 'eslint-plugin-import-x';
import kerfjs from 'eslint-plugin-kerfjs';
import simpleImportSort from 'eslint-plugin-simple-import-sort';
import tsdoc from 'eslint-plugin-tsdoc';
import tseslint from 'typescript-eslint';

export default tseslint.config(
  {
    ignores: ['dist/**', 'node_modules/**', 'test-results/**', 'playwright-report/**', 'scripts/**', 'eslint.config.mjs'],
  },
  {
    linterOptions: {
      reportUnusedDisableDirectives: 'error',
    },
  },
  eslint.configs.recommended,
  ...tseslint.configs.strictTypeChecked,
  {
    languageOptions: {
      parserOptions: {
        projectService: true,
        tsconfigRootDir: import.meta.dirname,
      },
    },
    plugins: {
      'simple-import-sort': simpleImportSort,
      import: importX,
      tsdoc,
    },
    rules: {
      '@typescript-eslint/consistent-type-imports': 'error',
      '@typescript-eslint/restrict-template-expressions': ['error', { allowNumber: true, allowBoolean: true, allow: [{ from: 'file', name: 'SafeHtml' }, { from: 'lib', name: 'URLSearchParams' }] }],
      '@typescript-eslint/strict-boolean-expressions': 'error',
      '@typescript-eslint/switch-exhaustiveness-check': 'error',
      'simple-import-sort/imports': 'error',
      'simple-import-sort/exports': 'error',
      'import/first': 'error',
      'import/newline-after-import': 'error',
      'import/no-duplicates': 'error',
      'tsdoc/syntax': 'warn',
    },
  },
  kerfjs.configs.recommended,
  {
    // High-volume legacy patterns require broader migrations. Every other strict
    // typed and Kerf correctness rule remains enforced, including in new files.
    rules: {
      '@typescript-eslint/no-non-null-assertion': 'off',
      '@typescript-eslint/strict-boolean-expressions': 'off',
      'kerfjs/prefer-attr-selector': 'off',
      'kerfjs/require-delegate-disposer': 'off',
    },
  },
  {
    // Typed request/response boundaries are intentionally narrowed by their callers.
    files: ['src/api.ts'],
    rules: {
      '@typescript-eslint/no-invalid-void-type': 'off',
      '@typescript-eslint/no-misused-spread': 'off',
      '@typescript-eslint/no-unsafe-argument': 'off',
      '@typescript-eslint/no-unsafe-member-access': 'off',
      '@typescript-eslint/no-unsafe-return': 'off',
      'import/first': 'off',
    },
  },
  {
    // These dev-only browser/file/CLI boundaries validate data at runtime. Keep the
    // exceptions file-local so equivalent mistakes remain errors everywhere else.
    files: ['src/dev-review/index.ts'],
    rules: {
      '@typescript-eslint/no-base-to-string': 'off',
      '@typescript-eslint/no-misused-promises': 'off',
      '@typescript-eslint/prefer-promise-reject-errors': 'off',
    },
  },
  {
    files: ['src/dev-review/server.ts'],
    rules: {
      '@typescript-eslint/no-unnecessary-condition': 'off',
      '@typescript-eslint/no-unnecessary-type-conversion': 'off',
    },
  },
  {
    files: ['src/dev-server.ts'],
    rules: { '@typescript-eslint/no-unsafe-return': 'off' },
  },
  {
    files: ['src/main.tsx'],
    rules: {
      '@typescript-eslint/no-unnecessary-condition': 'off',
      '@typescript-eslint/use-unknown-in-catch-callback-variable': 'off',
      'simple-import-sort/imports': 'off',
    },
  },
  {
    files: ['src/project-bridge.ts'],
    rules: { '@typescript-eslint/no-misused-spread': 'off' },
  },
  {
    // MarkdownPreview's renderer escapes raw HTML and constrains URL schemes before
    // the deliberately raw Kerf rendering boundary.
    files: ['src/components/markdown-preview.tsx'],
    rules: { 'kerfjs/no-raw-with-dynamic-arg': 'off' },
  },
  {
    files: ['src/ux-demo/app-shell-demo.tsx'],
    rules: { '@typescript-eslint/no-unnecessary-condition': 'off' },
  },
  {
    files: ['src/ux-demo/main.tsx'],
    rules: {
      '@typescript-eslint/no-unnecessary-condition': 'off',
      '@typescript-eslint/switch-exhaustiveness-check': 'off',
    },
  },
  {
    files: ['**/*.test.ts', '**/*.spec.ts', 'tests/**/*.ts'],
    rules: {
      '@typescript-eslint/consistent-type-imports': 'off',
      '@typescript-eslint/no-unsafe-argument': 'off',
      '@typescript-eslint/no-unsafe-assignment': 'off',
      '@typescript-eslint/no-unsafe-call': 'off',
      '@typescript-eslint/no-unsafe-member-access': 'off',
      '@typescript-eslint/no-unsafe-return': 'off',
      '@typescript-eslint/no-deprecated': 'off',
      '@typescript-eslint/require-await': 'off',
    },
  },
);
