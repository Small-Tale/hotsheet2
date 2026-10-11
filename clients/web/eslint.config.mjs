import eslint from '@eslint/js';
import stylistic from '@stylistic/eslint-plugin';
import importX from 'eslint-plugin-import-x';
import kerfjs from 'eslint-plugin-kerfjs';
import simpleImportSort from 'eslint-plugin-simple-import-sort';
import tsdoc from 'eslint-plugin-tsdoc';
import tseslint from 'typescript-eslint';

export default tseslint.config(
  {
    ignores: [
      'dist/**',
      'dist-host/**',
      'node_modules/**',
      'test-results/**',
      'playwright-report/**',
      'scripts/**',
      'eslint.config.mjs',
    ],
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
      '@stylistic': stylistic,
      'simple-import-sort': simpleImportSort,
      import: importX,
      tsdoc,
    },
    rules: {
      '@typescript-eslint/consistent-type-imports': 'error',
      '@typescript-eslint/restrict-template-expressions': [
        'error',
        {
          allowNumber: true,
          allowBoolean: true,
          allow: [
            { from: 'file', name: 'SafeHtml' },
            { from: 'lib', name: 'URLSearchParams' },
          ],
        },
      ],
      '@typescript-eslint/strict-boolean-expressions': 'error',
      '@typescript-eslint/switch-exhaustiveness-check': 'error',
      'simple-import-sort/imports': 'error',
      'simple-import-sort/exports': 'error',
      'import/first': 'error',
      'import/newline-after-import': 'error',
      'import/no-duplicates': 'error',
      'tsdoc/syntax': 'warn',
      // Childless JSX elements, native and custom, use the self-closing form (HS2-G5WBZ5).
      '@stylistic/jsx-self-closing-comp': ['error', { component: true, html: true }],
      // Prettier owns layout: no `prettier-ignore` escapes, and no minified code lines that
      // only an ignore could keep (HS2-PR1BST). Scope a lint exception with a block
      // `eslint-disable`/`eslint-enable` pair and a `--` reason instead.
      'no-warning-comments': ['error', { terms: ['prettier-ignore'], location: 'start' }],
      '@stylistic/max-len': [
        'error',
        {
          code: 200,
          ignoreComments: true,
          ignoreStrings: true,
          ignoreTemplateLiterals: true,
          ignoreRegExpLiterals: true,
          ignoreUrls: true,
        },
      ],
    },
  },
  kerfjs.configs.recommended,
  {
    // High-volume legacy patterns require broader migrations. Every other strict
    // typed and Kerf correctness rule remains enforced, including in new files.
    rules: {
      '@typescript-eslint/no-non-null-assertion': 'off',
      '@typescript-eslint/strict-boolean-expressions': 'off',
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
    // Every child process in src/ goes through src/child-process.ts, which strips inherited
    // repository-locating GIT_* variables so a git hook or `git bisect run` cannot redirect a spawned
    // git or Hot Sheet binary into the calling repository (HS2-T1H6NP).
    files: ['src/**/*.{ts,tsx,mts}'],
    ignores: ['src/child-process.ts'],
    rules: {
      '@typescript-eslint/no-restricted-imports': [
        'error',
        {
          paths: ['node:child_process', 'child_process'].map((name) => ({
            name,
            allowTypeImports: true,
            message: 'Spawn through src/child-process.ts so inherited GIT_DIR and friends are stripped (HS2-T1H6NP).',
          })),
        },
      ],
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
    files: ['src/ux-demo/main.tsx'],
    rules: {
      '@stylistic/array-bracket-spacing': ['error', 'never'],
      '@stylistic/arrow-spacing': 'error',
      '@stylistic/block-spacing': 'error',
      '@stylistic/comma-spacing': ['error', { before: false, after: true }],
      '@stylistic/computed-property-spacing': ['error', 'never'],
      '@stylistic/function-call-spacing': ['error', 'never'],
      '@stylistic/indent': [
        'error',
        2,
        {
          SwitchCase: 1,
          ignoredNodes: ['ConditionalExpression', 'ConditionalExpression *', 'TSUnionType', 'TSUnionType *'],
        },
      ],
      '@stylistic/key-spacing': 'error',
      '@stylistic/keyword-spacing': 'error',
      '@stylistic/max-statements-per-line': ['error', { max: 1 }],
      '@stylistic/object-curly-spacing': ['error', 'always'],
      '@stylistic/semi-spacing': 'error',
      '@stylistic/space-before-blocks': 'error',
      '@stylistic/space-in-parens': ['error', 'never'],
      '@stylistic/space-infix-ops': 'error',
      '@stylistic/template-curly-spacing': ['error', 'never'],
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
