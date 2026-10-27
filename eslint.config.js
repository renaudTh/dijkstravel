import { defineConfig } from 'eslint/config';
import tseslint from 'typescript-eslint';

export default defineConfig(
  { ignores: ['node_modules', 'data', 'target', 'editor/dist'] },
  {
    files: ['**/*.ts'],
    extends: [tseslint.configs.strictTypeChecked],
    languageOptions: { parserOptions: { projectService: true } },
    rules: {
      '@typescript-eslint/consistent-type-assertions': ['error', { assertionStyle: 'never' }],
      'no-restricted-syntax': [
        'error',
        {
          selector: ':matches(MethodDefinition, Property)[kind=/^[gs]et$/]',
          message: 'Pas de get/set : une vraie fonction.',
        },
      ],
    },
  },
);
