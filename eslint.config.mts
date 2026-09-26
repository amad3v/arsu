// ESLint v10 flat config for the SolidJS + TypeScript frontend.
//
// Sources of truth:
//   eslint                            10.x — https://eslint.org/docs/latest/
//   typescript-eslint                 8.x  — https://typescript-eslint.io/
//   eslint-plugin-solid               0.x  — https://github.com/solidjs-community/eslint-plugin-solid
//   eslint-plugin-import-x            4.x  — https://github.com/un-ts/eslint-plugin-import-x
//   eslint-import-resolver-typescript 4.x  — https://github.com/import-js/eslint-import-resolver-typescript
//
// Type-import split behaviour:
//   import { For, type Component } from '...'
//   becomes ↓
//   import { For } from '...'
//   import type { Component } from '...'
//
//   Three rules work together to guarantee this on every `eslint --fix` run:
//     1. @typescript-eslint/consistent-type-imports
//          prefer: 'type-imports', fixStyle: 'separate-type-imports'
//          → any import used only as a type gets pulled out into a top-level
//            `import type` statement (auto-fixable).
//     2. @typescript-eslint/no-import-type-side-effects
//          → if someone wrote `import { type A, type B } from '...'`, the
//            inline-type markers are collapsed into `import type { A, B }`.
//     3. import-x/consistent-type-specifier-style  ('prefer-top-level')
//          → mirrors rule 1 at the import-x layer, catching any edge cases
//            the TS-ESLint rule misses.

import path from 'node:path';

import js from '@eslint/js';
import unocss from '@unocss/eslint-config/flat';
import { defineConfig } from 'eslint/config';
import { createTypeScriptImportResolver } from 'eslint-import-resolver-typescript';
import { importX } from 'eslint-plugin-import-x';
import solid from 'eslint-plugin-solid/configs/typescript';
import globals from 'globals';
import tseslint from 'typescript-eslint';

/** Application code (browser, Solid). Covered by `tsconfig.app.json`. */
const appFiles = ['src/**/*.{ts,tsx}'];
/** Root build/tool configs (Node). Covered by `tsconfig.node.json`. */
const configFiles = ['*.config.{ts,mts}'];

export default [
  ...defineConfig(
    // ─── 1. Files to ignore ──────────────────────────────────────────────────
    { ignores: ['**/dist/**', '**/node_modules/**', 'src-tauri/**', 'target/**'] },
    unocss,

    // ─── 2. TypeScript — typed linting ───────────────────────────────────────
    // The Project Service resolves each file through the solution-style
    // `tsconfig.json` to the referenced project that includes it
    // (`tsconfig.app.json` for src, `tsconfig.node.json` for root configs).
    // Docs: https://typescript-eslint.io/getting-started/typed-linting
    {
      files: [...appFiles, ...configFiles],
      extends: [
        js.configs.recommended,
        // strictTypeChecked — superset of recommendedTypeChecked; catches more bugs
        // stylisticTypeChecked — consistent style rules that leverage type info
        tseslint.configs.strictTypeChecked,
        tseslint.configs.stylisticTypeChecked,
      ],
      languageOptions: {
        ecmaVersion: 'latest',
        sourceType: 'module',
        parserOptions: {
          projectService: true,
          tsconfigRootDir: import.meta.dirname,
        },
      },
      rules: {
        // ── Type-import separation (see header) ────────────────────────────────
        '@typescript-eslint/consistent-type-imports': [
          'error',
          {
            prefer: 'type-imports',
            fixStyle: 'separate-type-imports',
            disallowTypeAnnotations: true,
          },
        ],
        '@typescript-eslint/no-import-type-side-effects': 'error',

        // ── Correctness ────────────────────────────────────────────────────────
        // Prefer `unknown` over `any` in catch clauses.
        '@typescript-eslint/use-unknown-in-catch-callback-variable': 'error',
        // Require explicit return types on public API / module-boundary functions.
        '@typescript-eslint/explicit-module-boundary-types': 'error',
        // Disallow `as` casts that do nothing.
        '@typescript-eslint/no-unnecessary-type-assertion': 'error',
        // Catch unhandled Promises (await-or-void).
        '@typescript-eslint/no-floating-promises': 'error',
        // Prevent `void` used as a value type (confusing with functions).
        '@typescript-eslint/no-invalid-void-type': 'error',
        // strictTypeChecked enables this; allow the Solid idiom
        // `onClick={() => setX(...)}` where the arrow shorthand returns the
        // setter's value but the handler discards it.
        '@typescript-eslint/no-confusing-void-expression': [
          'error',
          { ignoreArrowShorthand: true },
        ],
        // Numbers interpolate unambiguously; everything else stays banned.
        '@typescript-eslint/restrict-template-expressions': ['error', { allowNumber: true }],
        // Ban `require()` — use ESM `import` instead.
        '@typescript-eslint/no-require-imports': 'error',
        // Disable the base rule; the TS-aware version handles it correctly.
        'no-unused-vars': 'off',
        '@typescript-eslint/no-unused-vars': [
          'error',
          {
            vars: 'all',
            args: 'after-used',
            caughtErrors: 'all',
            ignoreRestSiblings: true,
            // Conventional prefix to intentionally unused bindings.
            argsIgnorePattern: '^_',
            varsIgnorePattern: '^_',
            caughtErrorsIgnorePattern: '^_',
            // lets --fix actually delete unused imports.
            enableAutofixRemoval: {
              imports: true,
            },
          },
        ],
        // Disallow explicit `any`.
        '@typescript-eslint/no-explicit-any': 'error',
        // Prefer `interface` over `type` for object shapes (stylistic).
        '@typescript-eslint/consistent-type-definitions': ['error', 'interface'],
      },
    },

    // ─── 3. Application code runs in the browser ─────────────────────────────
    {
      files: appFiles,
      languageOptions: {
        globals: { ...globals.browser },
      },
      rules: {
        // The app has no logging channel; errors are surfaced in the UI.
        'no-console': 'error',
      },
    },

    // ─── 4. Root configs run in Node ─────────────────────────────────────────
    {
      files: configFiles,
      languageOptions: {
        globals: { ...globals.node },
      },
    },

    // ─── 5. import-x — module resolution & import hygiene ────────────────────
    // Docs: https://github.com/un-ts/eslint-plugin-import-x
    {
      files: [...appFiles, ...configFiles],
      plugins: {
        'import-x': importX,
      },
      settings: {
        // Vite virtual modules (no file on disk) — declare them as core modules
        // so `no-unresolved` does not flag them. UnoCSS injects this at build time.
        'import-x/core-modules': ['virtual:uno.css'],
        // resolver-next: the modern flat-config resolver API (import-x >= 4.5).
        // Pointed at the app project, which owns the `paths` aliases.
        'import-x/resolver-next': [
          createTypeScriptImportResolver({
            project: path.join(import.meta.dirname, 'tsconfig.app.json'),
            // Resolve @types/* even for packages that ship no JS source.
            alwaysTryTypes: true,
          }),
        ],
      },
      rules: {
        // Spread the flat/typescript preset rules (handles TS path aliases,
        // declaration files, etc.) — note: only rules, no parser override.
        ...importX.flatConfigs.typescript.rules,

        // ── Import correctness ─────────────────────────────────────────────────
        // No unresolvable imports (relies on the resolver above).
        'import-x/no-unresolved': 'error',
        // No named imports that don't exist in the target module.
        'import-x/named': 'error',
        // No circular dependencies.
        'import-x/no-cycle': 'error',
        // No importing a package that isn't in package.json.
        'import-x/no-extraneous-dependencies': 'error',
        // Collapse duplicate imports from the same source into one.
        'import-x/no-duplicates': ['error', { 'prefer-inline': false }],

        // ── Import style ───────────────────────────────────────────────────────
        // Type-import style mirror (see header).
        'import-x/consistent-type-specifier-style': ['error', 'prefer-top-level'],
        // Enforce a canonical import order:
        //   1. Node built-ins  2. External packages  3. Internal paths  4. Relatives
        'import-x/order': [
          'error',
          {
            groups: [
              'builtin',
              'external',
              'internal',
              'parent',
              'sibling',
              'index',
              'object',
              'type',
            ],
            'newlines-between': 'always',
            alphabetize: { order: 'asc', caseInsensitive: true },
          },
        ],
        // Require a newline after the last import statement.
        'import-x/newline-after-import': 'error',
      },
    },

    // ─── 6. Test files — relax selected rules ────────────────────────────────
    {
      files: ['src/**/*.test.{ts,tsx}'],
      rules: {
        // Test utilities routinely have non-null assertions and explicit any.
        '@typescript-eslint/no-explicit-any': 'off',
        '@typescript-eslint/no-non-null-assertion': 'off',
        // Module boundary types are noisy in test helpers.
        '@typescript-eslint/explicit-module-boundary-types': 'off',
        // Test diagnostics may use the console freely.
        'no-console': 'off',
      },
    },
  ),

  // ─── 7. Solid — reactivity & JSX correctness for application code ──────────
  // Solid's TypeScript preset (reactivity tracking, no-destructure, JSX rules),
  // layered on the typed parser from section 2, spread exactly as the plugin
  // documents. It sits outside `defineConfig()` because the plugin's rules are
  // typed with @typescript-eslint/utils' `RuleModule`, which is deliberately not
  // assignable to ESLint core's `RuleDefinition`
  // (typescript-eslint/typescript-eslint#11543, eslint-plugin-solid#178);
  // ESLint still validates this block at load time.
  // Docs: https://github.com/solidjs-community/eslint-plugin-solid
  { files: appFiles, ...solid },
];
