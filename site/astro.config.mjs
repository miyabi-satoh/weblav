// @ts-check
// weblav.amiiby.com の紹介・規約・プライバシーポリシー。
// 見た目は運営者のサイト (amiiby.com) の紹介ページと揃える。
import { defineConfig } from 'astro/config';
import { satteri } from '@astrojs/markdown-satteri';
import sitemap from '@astrojs/sitemap';
import { hastProse } from './src/lib/hast-prose.ts';
import { hastShowcase } from './src/lib/hast-showcase.ts';

export default defineConfig({
  site: 'https://weblav.amiiby.com',
  trailingSlash: 'always',
  integrations: [sitemap()],
  markdown: {
    // 引用符や ... を組版用の記号に変えない。アプリ (marked) の出力に合わせる
    processor: satteri({ hastPlugins: [hastProse, hastShowcase], features: { smartPunctuation: false } }),
  },
});
