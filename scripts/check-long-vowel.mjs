// 利用者に見える日本語の文言で、語末の長音を落とした外来語 (`フォルダ`・`ブラウザ` など) を
// 検出する (→ docs/ui.md「UI 全般」)。macOS などが長音なしで書く語は、書く人の手元の表記に引かれて戻りやすい。
// 決まりそのもの (-er・-or・-ar は付ける) は機械では見分けられないので、取り違えやすい語だけを並べる。
// Node で書いているのは check-generated.mjs と同じく Windows でも `just ci` から動かすため。

import { readdirSync, readFileSync, statSync } from 'node:fs';
import path from 'node:path';

const root = path.join(import.meta.dirname, '..');

// 利用者が読む文言の置き場。テスト・コメント・開発の文書は対象外。
const targets = ['frontend/messages/ja.json', 'docs/manual/ja', 'site/src', 'src/tray'];
const extensions = /\.(json|md|mdx|astro|ts|svelte|rs)$/;

const words = [
  'フォルダ',
  'ブラウザ',
  'ビューア',
  'サーバ',
  'ユーザ',
  'フィルタ',
  'ヘッダ',
  'フッタ',
  'コンピュータ',
  'エディタ',
  'プリンタ',
  'スキャナ',
  'モニタ',
  'プレイヤ',
  'パラメータ',
  'メンバ',
  'カレンダ',
  'マネージャ',
  'コントローラ'
];
const pattern = new RegExp(`(?:${words.join('|')})(?!ー)`, 'g');

function files(relative) {
  const absolute = path.join(root, relative);
  if (!statSync(absolute).isDirectory()) return [relative];
  return readdirSync(absolute, { recursive: true })
    .map((entry) => `${relative}/${entry.split(path.sep).join('/')}`)
    .filter((entry) => extensions.test(entry));
}

const violations = [];
for (const file of targets.flatMap(files)) {
  const lines = readFileSync(path.join(root, file), 'utf8').split('\n');
  lines.forEach((line, index) => {
    for (const match of line.matchAll(pattern)) {
      violations.push(`${file}:${index + 1}: ${match[0]}`);
    }
  });
}

if (violations.length > 0) {
  console.error('外来語の語末の長音を付けてください (→ docs/ui.md「UI 全般」):');
  for (const violation of violations) console.error(`  ${violation}`);
  process.exit(1);
}
