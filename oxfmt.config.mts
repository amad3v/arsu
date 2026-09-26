import { defineConfig } from 'oxfmt';

export default defineConfig({
  singleQuote: true,
  semi: true,
  trailingComma: 'all',
  printWidth: 100,
  // Written by pnpm, in its own format.
  ignorePatterns: ['pnpm-lock.yaml'],
});
