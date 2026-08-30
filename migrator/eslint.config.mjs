import eslint from '@eslint/js';

export default [
  { ignores: ['coverage/**', 'node_modules/**'] },
  eslint.configs.recommended,
  {
    files: ['**/*.mjs'],
    languageOptions: {
      ecmaVersion: 'latest',
      sourceType: 'module',
      globals: { Buffer: 'readonly', console: 'readonly', process: 'readonly' },
    },
  },
];
