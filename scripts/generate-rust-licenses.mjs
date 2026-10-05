// 配布物に入る Rust の依存のライセンス表示を、画面が読む JSON にする (→ docs/third-party-licenses.md)。
// cargo-about で抽出し、実際にリンクされるクレートに絞って差し替えを当ててから、
// 画面に出す形に整える (scripts/license-display.mjs。npm の分と共通)。

import { execFileSync } from 'node:child_process';
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { buildLicenseDisplay } from './license-display.mjs';

const OUTPUT = 'frontend/static/third-party-licenses/rust.json';
const TEXTS_DIR = 'license-texts';
const ABOUT_CONFIG = 'about.toml';

// 条文のファイルを同梱していないので、cargo-about が SPDX のひな形 (著作権者が空欄) で埋めるクレートの版と、
// 代わりに出す条文 (TEXTS_DIR に置いたもの)。条文は各クレートの .cargo_vcs_info.json が指す公開時のコミットから取った。
// - objc2.txt: madsmtm/objc2 の LICENSE.md (b4167b5・8852b42・8d214f5・7b1abfd で同じ) と MIT の条文。
//   上流は MIT で提供すると書くだけで、著作権表示をどこにも掲げていないので、著作権者の行は置かない。
// - dlopen2.txt: OpenByteDev/dlopen2 の LICENSE (cc80e4a)
// - tower-sessions.txt: maxcountryman/tower-sessions の LICENSE (b58463d)
// - tower-sessions-stores.txt: maxcountryman/tower-sessions-stores の LICENSE (b34a2f3)
// 差し替えは1つの版に本文1つ。その版が cargo-about から得た本文は全部落とすので、本文が複数要るクレート
// (`Apache-2.0 AND MIT` など) を足すときは、形を広げてから足す。
// 版まで書くのは、新しい版で上流が条文を同梱したり著作権者を変えたりしても、古い条文を黙って当てないため。
// 表に無い版が現れたら生成を止めるので、その版の条文を確かめてから版を足す。
const OVERRIDES = {
  'block2@0.6.2': { id: 'MIT', file: 'objc2.txt' },
  'objc2@0.6.4': { id: 'MIT', file: 'objc2.txt' },
  'objc2-encode@4.1.0': { id: 'MIT', file: 'objc2.txt' },
  'objc2-foundation@0.3.2': { id: 'MIT', file: 'objc2.txt' },
  'dlopen2@0.8.2': { id: 'MIT', file: 'dlopen2.txt' },
  'tower-sessions-core@0.14.0': { id: 'MIT', file: 'tower-sessions.txt' },
  'tower-sessions-sqlx-store@0.15.0': {
    id: 'MIT',
    file: 'tower-sessions-stores.txt'
  }
};
const OVERRIDDEN_NAMES = new Set(Object.keys(OVERRIDES).map((key) => key.replace(/@[^@]+$/, '')));

// cargo-about が内部で呼ぶ `cargo metadata` は、どのプラットフォーム向けの依存も手元に要求する。
// Windows でビルドしただけのレジストリには Linux 向けの libxdo などが無く、`--frozen` のまま
// 取りに行って落ちる。取るのは lockfile が指すものだけなので、先に取っても結果は変わらない。
execFileSync('cargo', ['fetch', '--locked'], { stdio: 'inherit' });

// 生成そのものは `--frozen` (= `--locked --offline`) でネットを見ず、手元のレジストリだけから作る。
// ネットワークを見ると、結果が環境で変わりうるため。
//
// 出力は標準出力ではなく `-o` で受ける。cargo-about は先祖のプロセスに PowerShell がいると
// 標準出力への書き出しを拒み、`-o` を使うよう促して終わる (0.9.2 の `is_powershell_parent`)。
// 間に node や just が挟まっていても親を遡って引っかかるので、どの OS でも `-o` を使う。
const workDir = mkdtempSync(join(tmpdir(), 'weblav-licenses-'));
let about;
try {
  const aboutJson = join(workDir, 'about.json');
  const args = ['about', 'generate', '--format', 'json', '--frozen', '-o', aboutJson];
  execFileSync('cargo', args, { stdio: 'inherit' });
  about = JSON.parse(readFileSync(aboutJson, 'utf8'));
} finally {
  // 後始末の失敗で、本来のエラーを覆い隠さない。Windows では子プロセスが書いた直後の削除が
  // EPERM・EBUSY で落ちることがある (ウイルス対策の走査やハンドルの解放待ち) ので、少し待って繰り返す。
  try {
    rmSync(workDir, {
      recursive: true,
      force: true,
      maxRetries: 3,
      retryDelay: 100
    });
  } catch {
    // 消せなくても OS の一時領域なので放っておく。
  }
}

/** @type {Map<string, import('./license-display.mjs').ExtractedPackage>} */
const extracted = new Map();
for (const { package: crate } of about.crates) {
  extracted.set(`${crate.name}@${crate.version}`, {
    name: crate.name,
    version: crate.version,
    license: crate.license ?? '',
    repository: httpOrNull(crate.repository),
    texts: []
  });
}
for (const license of about.licenses) {
  for (const { crate } of license.used_by) {
    const pkg = extracted.get(`${crate.name}@${crate.version}`);
    if (!pkg) throw new Error(`${crate.name}@${crate.version} が crates に無い`);
    pkg.texts.push({ id: license.id, name: license.name, text: license.text });
  }
}

const licenseNames = new Map(about.licenses.map((license) => [license.id, license.name]));
for (const [key, { id, file }] of Object.entries(OVERRIDES)) {
  const pkg = extracted.get(key);
  // 差し替え先が一覧から消えていたら、表を古いまま残さない。
  if (!pkg) throw new Error(`OVERRIDES の ${key} が一覧に無い`);
  // Windows の checkout で CRLF になっても、生成物が OS で変わらないよう LF に揃える。
  const text = readFileSync(join(TEXTS_DIR, file), 'utf8').replace(/\r\n/g, '\n');
  pkg.texts = [{ id, name: licenseNames.get(id) ?? id, text }];
}
for (const [key, pkg] of extracted) {
  if (OVERRIDDEN_NAMES.has(pkg.name) && !(key in OVERRIDES)) {
    throw new Error(`${key} の条文を確かめて OVERRIDES に足す (表にあるのは別の版)`);
  }
}

// 表示するのは、配る実行ファイルに実際にリンクされるクレートだけ。cargo-about はビルドのときにだけ動く
// proc-macro とその依存 (syn・serde_derive など) も数えるが、それらは配らない。
// リンクされるのに cargo-about の一覧に無いものがあれば、数え漏れなので止める。
const linked = linkedCrates();
const missing = [...linked].filter((key) => !extracted.has(key));
if (missing.length > 0) {
  throw new Error(`リンクされるのに一覧に無いクレートがある: ${missing.join(', ')}`);
}
const display = buildLicenseDisplay(
  [...extracted.values()].filter((pkg) => linked.has(`${pkg.name}@${pkg.version}`))
);

mkdirSync(dirname(OUTPUT), { recursive: true });
writeFileSync(OUTPUT, JSON.stringify(display, null, '\t') + '\n');

/**
 * about.toml の `targets` のどれかで、実際にリンクされるクレート (`名前@版`)。
 * `cargo tree` で、通常の依存だけを、proc-macro を除いて辿る。手元のレジストリだけを見る。
 */
function linkedCrates() {
  const config = readFileSync(ABOUT_CONFIG, 'utf8');
  const targets = [
    ...(config.match(/^targets\s*=\s*\[([^\]]*)\]/m)?.[1] ?? '').matchAll(/"([^"]+)"/g)
  ].map((m) => m[1]);
  if (targets.length === 0) throw new Error(`${ABOUT_CONFIG} の targets を読めない`);
  const keys = new Set();
  for (const target of targets) {
    const args = ['tree', '--locked', '--offline', '--target', target];
    args.push('-e', 'normal,no-proc-macro', '--prefix', 'none', '--format', '{p}');
    const out = execFileSync('cargo', args, { encoding: 'utf8' });
    for (const line of out.split(/\r?\n/)) {
      const m = line.match(/^(\S+) v(\S+)(.*)$/);
      // weblav 自身のようにパスで参照するパッケージ (`(/…)`・`(C:\…)`・UNC の `(\\server\…)`・`(\\?\C:\…)`) は除く。
      if (!m || /\((?:[A-Za-z]:\\|\\\\|\/)/.test(m[3])) continue;
      keys.add(`${m[1]}@${m[2]}`);
    }
  }
  return keys;
}

/** @param {unknown} url */
function httpOrNull(url) {
  // 画面はリンクにするので、URL でないものは出さない (npm 側の `repositoryUrl` と同じ扱い)。
  // 末尾の `.git` はリポジトリの取得用の書き方なので、ブラウザで開くページの URL にする。
  return typeof url === 'string' && /^https?:\/\//.test(url) ? url.replace(/\.git$/, '') : null;
}
