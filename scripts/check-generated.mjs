// Node で書いているのは、pre-commit hook と `just ci` の両方から
// 呼ぶ検査を Windows でも実行できるようにするため (justfile の windows-shell は cmd.exe で
// POSIX シェル構文が使えず、lefthook.yml に書いた sh スクリプトも Windows では
// 引用符が崩れて構文エラーになる)。

import { execFileSync } from 'node:child_process';

const [recipe, ...targets] = process.argv.slice(2);
if (!recipe || targets.length === 0) {
  console.error('usage: node scripts/check-generated.mjs <recipe> <path>...');
  process.exit(2);
}

function hasUnstagedChanges() {
  try {
    execFileSync('git', ['diff', '--quiet', '--', ...targets], { stdio: 'ignore' });
    return false;
  } catch (err) {
    // 差分があるときの終了コードは1。それ以外はgit自体の失敗なので伝播させる。
    if (err.status === 1) return true;
    throw err;
  }
}

/**
 * 対象配下の未追跡ファイル。新規クエリで生まれる `.sqlx/query-<hash>.json` や、
 * `git rm --cached` 直後の再生成物は未追跡になり diff に出ないため、差分とは別に見る。
 */
function untrackedTargets() {
  const stdout = execFileSync(
    'git',
    ['ls-files', '--others', '--exclude-standard', '--', ...targets],
    { encoding: 'utf8' }
  );
  return stdout.trim();
}

if (hasUnstagedChanges() || untrackedTargets()) {
  console.error(
    `${targets.join(' / ')} が古い状態です。'just ${recipe}' の結果を commit に含めてください。`
  );
  process.exit(1);
}
