// 開発用の DB に、まだ当てていないマイグレーションを当てる (`just dev-db-migrate`)。
// pull の後に lefthook (post-merge) が流す。sqlx のマクロはビルドの時点で開発用の DB を見るので、
// 当て忘れると `no such table` でビルドやリントが止まる (→ DEVELOPMENT.md)。
// Node で書いているのは check-generated.mjs と同じく Windows でも動かすため。
//
// pull を止めないよう、当てられないとき (DB が無い・sqlx-cli が無い・worktree の中) は知らせるだけで 0 で終える。

import { execFileSync, spawnSync } from 'node:child_process';
import { existsSync } from 'node:fs';

const url = process.env.DATABASE_URL;
const dbPath = url?.replace(/^sqlite:/, '');
if (!dbPath || !existsSync(dbPath)) process.exit(0); // 開発用の DB を作っていない clone

// worktree のブランチのマイグレーションを、元の clone と共有している DB に当てない。
const gitPath = (flag) =>
  execFileSync('git', ['rev-parse', '--path-format=absolute', flag], { encoding: 'utf8' }).trim();
if (gitPath('--git-dir') !== gitPath('--git-common-dir')) process.exit(0);

// Windows の cargo は PATH の解決に shell が要る。
const result = spawnSync('cargo', ['sqlx', 'migrate', 'run'], {
  stdio: 'inherit',
  shell: process.platform === 'win32'
});
if (result.status !== 0) {
  console.warn(
    '開発用の DB にマイグレーションを当てられませんでした。sqlx-cli が無ければ入れてから `just dev-db-migrate` を流してください:\n' +
      '  cargo install sqlx-cli --no-default-features --features sqlite'
  );
}
