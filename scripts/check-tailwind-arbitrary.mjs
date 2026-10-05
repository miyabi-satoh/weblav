// Tailwind の任意値クラス (`text-[13px]`・`h-(--x)`・`[mask-type:luminance]` など) を
// 使っていないかを検査する (→ AGENTS.md「コードの規約」)。`data-[size=default]:` のような
// 任意値の variant は対象外。
// Node で書いているのは check-generated.mjs と同じく Windows でも `just ci` から動かすため。

import { readdirSync, readFileSync } from 'node:fs';
import path from 'node:path';

const root = path.join(import.meta.dirname, '..', 'frontend', 'src');

// shadcn-svelte の部品と生成物は対象外 (eslint.config.js の ignores と揃える)。
const ignored = ['lib/components/ui/', 'lib/paraglide/', 'lib/api/schema.d.ts'];

// 1つ目が任意値 (`text-[13px]`・`h-(--x)`)、2つ目が任意プロパティ (`[mask-type:luminance]`)。
// 直後が `:` (または `/名前:`) なら variant なので除く。直前が英数字や `-` なら、より長いクラスの途中。
const pattern =
  /(?<![\w-])!?(?:-?[a-z][\w-]*-(?:\[[^\]\s]+\]|\([^)\s]+\))|\[-?[a-z-]+:[^\]\s]+\])(?![\w:\]-])(?!\/[\w-]+:)/g;

const violations = [];
for (const entry of readdirSync(root, { recursive: true })) {
  const relative = entry.split(path.sep).join('/');
  if (!/\.(svelte|ts|js)$/.test(relative)) continue;
  if (ignored.some((prefix) => relative.startsWith(prefix))) continue;

  const lines = readFileSync(path.join(root, entry), 'utf8').split('\n');
  lines.forEach((line, index) => {
    for (const match of line.matchAll(pattern)) {
      violations.push(`frontend/src/${relative}:${index + 1}: ${match[0]}`);
    }
  });
}

if (violations.length > 0) {
  console.error('Tailwind の任意値クラスは使わず、近い標準の段階に丸めてください:');
  for (const violation of violations) console.error(`  ${violation}`);
  process.exit(1);
}
