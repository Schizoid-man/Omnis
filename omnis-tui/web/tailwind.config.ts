import type { Config } from 'tailwindcss'

const config: Config = {
  darkMode: ['class'],
  content: [
    './src/app/**/*.{ts,tsx}',
    './src/components/**/*.{ts,tsx}',
    './src/lib/**/*.{ts,tsx}',
  ],
  theme: {
    extend: {
      fontFamily: {
        syne:    ['var(--font-syne)',    'Syne',             'system-ui', 'sans-serif'],
        jakarta: ['var(--font-jakarta)', 'Plus Jakarta Sans', 'system-ui', 'sans-serif'],
        mono:    ['var(--font-mono)',    'JetBrains Mono',   'Fira Code', 'monospace'],
        /* legacy alias kept for any code still referencing font-manrope */
        manrope: ['var(--font-jakarta)', 'Plus Jakarta Sans', 'system-ui', 'sans-serif'],
      },
      colors: {
        background:              'hsl(var(--background))',
        foreground:              'hsl(var(--foreground))',
        card:                    'hsl(var(--card))',
        'card-foreground':       'hsl(var(--card-foreground))',
        border:                  'hsl(var(--border))',
        input:                   'hsl(var(--input))',
        primary: {
          DEFAULT:    'hsl(var(--primary))',
          foreground: 'hsl(var(--primary-foreground))',
        },
        secondary: {
          DEFAULT:    'hsl(var(--secondary))',
          foreground: 'hsl(var(--secondary-foreground))',
        },
        muted: {
          DEFAULT:    'hsl(var(--muted))',
          foreground: 'hsl(var(--muted-foreground))',
        },
        accent: {
          DEFAULT:    'hsl(var(--accent))',
          foreground: 'hsl(var(--accent-foreground))',
        },
        destructive: {
          DEFAULT:    'hsl(var(--destructive))',
          foreground: 'hsl(var(--destructive-foreground))',
        },
      },
      borderRadius: {
        sm:    '0.5rem',
        md:    '0.65rem',
        lg:    '0.85rem',
        xl:    '1rem',
        '2xl': '1.3rem',
        '3xl': '1.75rem',
        '4xl': '2rem',
      },
    },
  },
  plugins: [],
}

export default config
