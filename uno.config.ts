import {
  defineConfig,
  presetIcons,
  presetWind4,
  transformerDirectives,
  transformerVariantGroup,
} from 'unocss';
import { theme as wind4Theme } from 'unocss/preset-wind4';

import type { IconifyJSON } from '@iconify/types';

export default defineConfig({
  content: {
    pipeline: {
      include: [
        // UnoCSS's default filter, widened from `[jt]sx` to `[jt]sx?` so that
        // class names in plain .ts/.js modules (hooks, data files) are extracted.
        /\.(vue|svelte|[jt]sx?|mdx?|astro|elm|php|phtml|html)($|\?)/,
      ],
    },
  },
  presets: [
    // The preset's own reset (preflight) is the only one loaded.
    //
    // `theme: true` emits every theme variable, in the theme's order. The
    // default, on demand, emits the used ones in the order the build happens
    // to meet them, which differs from one build to the next: the CSS, and so
    // the app, would never be reproducible.
    presetWind4({ dark: 'class', preflights: { theme: true } }),
    presetIcons({
      scale: 1.2,
      // preset-icons sizes icons with `width/height: 1em` but emits no `display`,
      // so an icon that is NOT a flex child (e.g. a checkbox indicator inside a
      // block element) stays `display: inline`, ignores width/height, and renders
      // 0×0/invisible. `inline-block` makes every `i-*` icon size correctly in any
      // context; `vertical-align: middle` keeps it centred on the text baseline.
      extraProperties: { display: 'inline-block', 'vertical-align': 'middle' },
      collections: {
        // Phosphor icon set, consumed as `i-ph-*` classes. Loaded lazily so the
        // full JSON never lands in the app bundle.
        ph: () => import('@iconify-json/ph/icons.json').then((m) => m.default as IconifyJSON),
      },
    }),
  ],
  transformers: [transformerVariantGroup(), transformerDirectives()],
  theme: {
    font: {
      // Self-hosted: @fontsource-variable/source-code-pro is imported by
      // src/index.tsx and bundled by Vite. The app never fetches fonts at
      // runtime; the preset's system stack is the fallback.
      mono: `'Source Code Pro Variable',${wind4Theme.font.mono}`,
    },
    // Every colour is a token from src/index.css, so light and dark switch together.
    colors: {
      bg: {
        app: 'var(--color-bg-app)',
        card: 'var(--color-bg-card)',
        input: 'var(--color-bg-input)',
        selected: 'var(--color-bg-selected)',
        hover: 'var(--color-bg-hover)',
        pressed: 'var(--color-bg-pressed)',
      },
      segment: {
        track: 'var(--color-segment-track)',
        thumb: 'var(--color-segment-thumb)',
      },
      text: {
        DEFAULT: 'var(--color-text)',
        muted: 'var(--color-text-muted)',
      },
      border: {
        DEFAULT: 'var(--color-border)',
        strong: 'var(--color-border-strong)',
      },
      focus: 'var(--color-focus)',
      primary: {
        DEFAULT: 'var(--color-primary)',
        hover: 'var(--color-primary-hover)',
        pressed: 'var(--color-primary-pressed)',
      },
      danger: {
        DEFAULT: 'var(--color-danger)',
        hover: 'var(--color-danger-hover)',
        pressed: 'var(--color-danger-pressed)',
      },
      warning: {
        DEFAULT: 'var(--color-warning)',
        bg: 'var(--color-warning-bg)',
        border: 'var(--color-warning-border)',
        text: 'var(--color-warning-text)',
      },
      success: {
        bg: 'var(--color-success-bg)',
        border: 'var(--color-success-border)',
        text: 'var(--color-success-text)',
      },
      error: {
        bg: 'var(--color-error-bg)',
        border: 'var(--color-error-border)',
        text: 'var(--color-error-text)',
      },
      info: {
        bg: 'var(--color-info-bg)',
        border: 'var(--color-info-border)',
        text: 'var(--color-info-text)',
      },
    },
  },
  rules: [
    // What a button's states change, and nothing else: its colours. Quick, on
    // the strong ease-out from src/motion.css. (A rule because UnoCSS has no
    // arbitrary list form of `transition-*`.)
    [
      'transition-press',
      {
        'transition-property': 'color, background-color, border-color',
        'transition-duration': '150ms',
        'transition-timing-function': 'var(--ease-out-strong)',
      },
    ],
  ],
  shortcuts: {
    // Every pressable control, styled like a desktop toolkit's button rather
    // than a web page's: no outline ring (keyboard focus tints the button as
    // hover does, set per variant below), a darker fill while pressed instead
    // of a shrink, and the disabled look. `aria-disabled` gets the same look as
    // the native `disabled` attribute, for controls that must stay focusable
    // while transiently unavailable (their handler guards the action instead).
    pressable:
      'outline-none transition-press disabled:(pointer-events-none opacity-50) aria-disabled:(pointer-events-none opacity-50)',
    'btn-base':
      'pressable inline-flex items-center justify-center gap-2 rounded-md px-3 py-2 text-sm font-medium',
    btn: 'btn-base border border-border bg-bg-card text-text shadow-sm hover:bg-bg-hover focus-visible:bg-bg-hover active:bg-bg-pressed',
    'btn-primary':
      'btn-base border border-transparent bg-primary text-white shadow-sm hover:bg-primary-hover focus-visible:bg-primary-hover active:bg-primary-pressed',
    'btn-danger':
      'btn-base border border-transparent bg-danger text-white shadow-sm hover:bg-danger-hover focus-visible:bg-danger-hover active:bg-danger-pressed',
    // Borderless, for controls inside a list row.
    'btn-ghost':
      'pressable inline-flex items-center justify-center gap-1.5 rounded-md text-text hover:bg-bg-hover focus-visible:bg-bg-hover active:bg-bg-pressed',
    input:
      'select-text w-full rounded-md border border-border-strong bg-bg-input text-text px-3 py-2 text-sm shadow-sm outline-none transition-colors focus:(border-focus ring-1 ring-focus) placeholder:text-text-muted',
    label: 'mb-1 block text-sm font-medium text-text',
    card: 'rounded-xl border border-border bg-bg-card p-4 shadow-sm',
    error: 'rounded-md border border-error-border bg-error-bg px-3 py-2 text-sm text-error-text',
    success:
      'rounded-md border border-success-border bg-success-bg px-3 py-2 text-sm text-success-text',
    info: 'rounded-md border border-info-border bg-info-bg px-3 py-2 text-sm text-info-text',
    warning:
      'rounded-md border border-warning-border bg-warning-bg px-3 py-2 text-sm text-warning-text',
    subtle: 'text-sm text-text-muted',
    kbd: 'rounded border border-border bg-bg-app px-1.5 font-mono text-xs text-text-muted',
    // Ark menus (theme, settings, a row's actions).
    'menu-content':
      'z-50 min-w-48 rounded-lg border border-border bg-bg-card p-1 shadow-lg outline-none',
    'menu-item':
      'flex items-center gap-2 rounded-md px-2 py-1.5 text-sm outline-none data-[highlighted]:bg-bg-hover',
    'menu-group-label': 'px-2 pb-1 pt-2 text-xs font-medium text-text-muted',
    'menu-separator': 'my-1 border-border',
  },
});
