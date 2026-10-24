// just を起動するたびに、打ったレシピと呼び手を data/just-usage.tsv に1行足す (justfile の `_usage`)。
// どのレシピを実際に使っているかを、マシンごとに数えるため。記録するのは data/just-usage.tsv を
// 先に作ったマシンだけ (justfile が有無を見る)。
// 呼び手 (人の端末・エージェント・lefthook) は just の親をたどった名前で分かる。
// Node で書いているのは check-generated.mjs と同じく Windows でも動かすため。
//
// just を止めないよう、何が起きても何も出さずに 0 で終える。

import { execFileSync } from 'node:child_process';
import { appendFileSync } from 'node:fs';
import { hostname } from 'node:os';
import path from 'node:path';

// just の上に記録する呼び手の数。端末 (bash < herdr) やエージェント (bash < claude)、
// lefthook (sh < lefthook < git) の見分けがつく深さ。
const CALLER_DEPTH = 4;

/** pid ごとの親と名前と引数。 */
function processTable() {
  const table = new Map();
  if (process.platform === 'win32') {
    const json = execFileSync(
      'powershell',
      [
        '-NoProfile',
        '-Command',
        'Get-CimInstance Win32_Process | Select-Object ProcessId,ParentProcessId,Name,CommandLine | ConvertTo-Json -Compress'
      ],
      {
        encoding: 'utf8',
        stdio: ['ignore', 'pipe', 'ignore'],
        maxBuffer: 64 * 1024 * 1024
      }
    );
    for (const p of JSON.parse(json)) {
      table.set(p.ProcessId, {
        ppid: p.ParentProcessId,
        name: p.Name.replace(/\.exe$/i, ''),
        args: p.CommandLine ?? ''
      });
    }
    return table;
  }
  const out = execFileSync('ps', ['-A', '-o', 'pid=,ppid=,comm=,args='], {
    encoding: 'utf8',
    stdio: ['ignore', 'pipe', 'ignore']
  });
  for (const line of out.split('\n')) {
    const m = line.trim().match(/^(\d+)\s+(\d+)\s+(\S+)\s*(.*)$/);
    if (m)
      table.set(Number(m[1]), {
        ppid: Number(m[2]),
        name: path.basename(m[3]),
        args: m[4]
      });
  }
  return table;
}

try {
  const table = processTable();
  // 間に just が backtick を流すシェル (sh・cmd) が挟まるので、名前で just を探す。
  let pid = process.ppid;
  while (table.has(pid) && table.get(pid).name !== 'just') pid = table.get(pid).ppid;
  const just = table.get(pid);
  if (just) {
    // 1つめの語は just 自身のパス。Windows の CommandLine は引用符で囲まれていることがある。
    const recipe = just.args.replace(/^("[^"]*"|\S+)\s*/, '');
    const callers = [];
    for (let p = just.ppid; table.has(p) && callers.length < CALLER_DEPTH; p = table.get(p).ppid) {
      callers.push(table.get(p).name);
    }
    const commonDir = execFileSync(
      'git',
      ['rev-parse', '--path-format=absolute', '--git-common-dir'],
      {
        encoding: 'utf8',
        stdio: ['ignore', 'pipe', 'ignore']
      }
    ).trim();
    const row = [new Date().toISOString(), hostname(), recipe || '(default)', callers.join(' < ')];
    appendFileSync(
      path.join(path.dirname(commonDir), 'data', 'just-usage.tsv'),
      row.join('\t') + '\n'
    );
  }
} catch {
  // 記録できなくても just は続ける。
}
