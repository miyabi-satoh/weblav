// 利用者に見える日本語の文言で、括弧の書き方が決まり (半角の括弧、外側の文字との間に空白) から
// 外れたものを検出する (→ docs/ui.md「UI 全般」)。全角の括弧は書く人や訳した AI の手元の表記に引かれて混ざりやすい。
// Node で書いているのは check-long-vowel.mjs と同じく Windows でも `just ci` から動かすため。

import { readdirSync, readFileSync, statSync } from 'node:fs';
import path from 'node:path';

const root = path.join(import.meta.dirname, '..');

// 紹介のサイト (site/) は、文章の書き方を amiiby-site の決まりに揃えるので対象外。
// タスクトレイ (src/tray) は全角だけを見る。空白の検査は、コードのコメントの括弧まで拾うため。
const targets = [
  { path: 'frontend/messages/ja.json', spacing: true },
  { path: 'account-server/src/i18n.ts', spacing: true },
  { path: 'docs/manual/ja', spacing: true },
  { path: 'src/tray', spacing: false }
];
const extensions = /\.(json|md|ts|rs)$/;

const fullWidth = /[（）]/g;
// 和文の文字に半角の括弧が直に接しているもの。「」などの約物との間には空白を入れない。
const tight = /[\p{sc=Han}\p{sc=Hiragana}\p{sc=Katakana}ー]\(|\)[\p{sc=Han}\p{sc=Hiragana}\p{sc=Katakana}ー]/gu;
// Markdown のリンクの `](url)` は括弧の表記ではないので除く。
const markdownLink = /\]\([^)]*\)/g;

function files(relative) {
  const absolute = path.join(root, relative);
  if (!statSync(absolute).isDirectory()) return [relative];
  return readdirSync(absolute, { recursive: true })
    .map((entry) => `${relative}/${entry.split(path.sep).join('/')}`)
    .filter((entry) => extensions.test(entry));
}

const violations = [];
for (const target of targets) {
  for (const file of files(target.path)) {
    const lines = readFileSync(path.join(root, file), 'utf8').split('\n');
    lines.forEach((line, index) => {
      for (const match of line.matchAll(fullWidth)) {
        violations.push(`${file}:${index + 1}: ${match[0]}`);
      }
      if (!target.spacing) return;
      for (const match of line.replace(markdownLink, ']').matchAll(tight)) {
        violations.push(`${file}:${index + 1}: ${match[0]}`);
      }
    });
  }
}

if (violations.length > 0) {
  console.error('括弧は半角で書き、外側の和文との間に半角の空白を入れてください (→ docs/ui.md「UI 全般」):');
  for (const violation of violations) console.error(`  ${violation}`);
  process.exit(1);
}
