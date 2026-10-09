// 使い捨ての backend (`target/debug/weblav-service`) を起動・停止する。
// `weblav` はトレイを出すので、トレイを出さないサーバーだけの exe を使う。
// `just spec` (generate-spec.ts)・`just e2e-local` (run-e2e.ts)・`just manual-shots` (manual-shots.ts) で共有する。
//
// 一時 WEBLAV_HOME と空きポートで起動するので、開発用 DB にも、並行して動いている
// 開発サーバー (:3000) にも影響しない。呼び出し側は必ず `stop()` を呼ぶこと。

import { spawn, type ChildProcess } from 'node:child_process';
import { copyFileSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import net from 'node:net';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { DatabaseSync } from 'node:sqlite';
import { TEST_ADMIN } from './test-account.ts';

const REPO_ROOT = path.resolve(import.meta.dirname, '../..');
const BIN_PATH = path.join(
	REPO_ROOT,
	'target',
	'debug',
	process.platform === 'win32' ? 'weblav-service.exe' : 'weblav-service'
);

export interface Backend {
	baseURL: string;
	/** マイグレーション済みの DB ファイル。起動を待ってから返すので、すぐ書き込める。 */
	dbPath: string;
	/** 起動からの標準出力・標準エラー。失敗の原因を出すときに使う。 */
	log: () => string;
	/** プロセスの終了を待ってから一時ディレクトリを消す。何度呼んでもよい。 */
	stop: () => Promise<void>;
}

/**
 * 起動した backend の DB を開いて `fn` に渡し、終わったら閉じる。
 *
 * sqlite3 の CLI ではなく node:sqlite を使うのは、Windows の実機に CLI を入れなくて済むようにするため。
 * backend が起動直後に期限切れセッションを消していることがあるので、ロックは待つ。
 */
export function withDatabase<T>(dbPath: string, fn: (db: DatabaseSync) => T): T {
	const db = new DatabaseSync(dbPath, { timeout: 5000 });
	try {
		return fn(db);
	} finally {
		db.close();
	}
}

// OSに空きポートを選ばせる。固定ポートだと、既に何かが同じポートで listen していた場合
// waitForPort がその別プロセスの応答を「起動成功」と誤判定してしまう
// (新規プロセス自身はbind失敗で終了しているのに、一時DBへの書き込みが無関係な別プロセスに
// 向かってしまう)。
async function getFreePort(): Promise<number> {
	return new Promise((resolve, reject) => {
		const server = net.createServer();
		server.on('error', reject);
		server.listen(0, '127.0.0.1', () => {
			const address = server.address();
			if (address && typeof address === 'object') {
				const port = address.port;
				server.close(() => resolve(port));
			} else {
				server.close();
				reject(new Error('空きポートの確保に失敗しました'));
			}
		});
	});
}

// 動的ポート確保だけでも大半の競合は防げるが、念のため子プロセスの早期終了(bind失敗等)も
// 監視し、health応答を待たずに検知できるようにする。
async function waitForPort(url: string, timeoutMs: number, proc: ChildProcess): Promise<void> {
	const deadline = Date.now() + timeoutMs;
	let exitInfo: string | null = null;
	proc.once('exit', (code, signal) => {
		exitInfo = `code=${code} signal=${signal}`;
	});
	while (Date.now() < deadline) {
		if (exitInfo !== null) {
			throw new Error(`backendプロセスが起動前に終了しました(${exitInfo})`);
		}
		try {
			const res = await fetch(url);
			if (res.status < 500) return;
		} catch {
			// まだ起動していない。リトライする。
		}
		await new Promise((r) => setTimeout(r, 200));
	}
	throw new Error(`backendが${timeoutMs}ms以内に起動しませんでした: ${url}`);
}

// SIGTERM送信後、プロセスが実際に終了するまで待つ(掴んでいるDBファイルを残したまま
// 一時ディレクトリを削除しないため)。無反応な場合に備えてタイムアウトでSIGKILLする。
async function killAndWait(proc: ChildProcess, timeoutMs = 5000): Promise<void> {
	if (proc.exitCode !== null || proc.signalCode !== null) return;
	await new Promise<void>((resolve) => {
		const timer = setTimeout(() => proc.kill('SIGKILL'), timeoutMs);
		proc.once('exit', () => {
			clearTimeout(timer);
			resolve();
		});
		proc.kill('SIGTERM');
	});
}

/** 管理者 (test-account.ts の TEST_ADMIN) を1人入れる。初回セットアップを通さずにログインできる。 */
export function insertTestAdmin(dbPath: string): void {
	withDatabase(dbPath, (db) =>
		db
			.prepare(
				"INSERT INTO users (username, password_hash, role, recovery_code_hash) VALUES (?, ?, 'admin', ?)"
			)
			.run(TEST_ADMIN.username, TEST_ADMIN.passwordHash, TEST_ADMIN.recoveryCodeHash)
	);
}

/**
 * 起動したまま、まだ止めていない backend の片付け。
 * 1つのスクリプトが続けて複数を起動する (→ run-e2e.ts) と、後のものの起動待ちで中断されたとき、
 * 先に起動したものは呼び出し側の finally に届かない (下の `onInterrupt` が終了させるため)。まとめて片付ける。
 */
const liveBackends = new Set<() => Promise<void>>();

/**
 * backend を起動し、health が応答するまで待つ。起動に失敗したら、ログを出して片付けてから投げる。
 * `tmpPrefix` は一時 WEBLAV_HOME の名前の頭 (どのスクリプトの残骸か分かるようにする)。
 * `pro: false` なら Free で動かす (Free の画面を撮るとき)。
 */
export async function startBackend(
	tmpPrefix: string,
	{ pro = true }: { pro?: boolean } = {}
): Promise<Backend> {
	const port = await getFreePort();
	const home = mkdtempSync(path.join(tmpdir(), tmpPrefix));
	writeFileSync(
		path.join(home, 'config.toml'),
		`[server]\nbind = "127.0.0.1"\nport = ${port}\n\n[log]\nfilter = "info"\noutput = "stdout"\n\n[session]\nsecret = ""\nsecure_cookie = false\nexpiry_days = 14\n`
	);
	// 既定は Pro。e2e はテストごとにコンテンツを作るので、並べて流すと Free の上限に当たる。
	// 開発版だけが信じる鍵で署名したもの (→ src/pro.rs)。確かめに行かないよう、最後に確かめた日時を先に置いてある。
	if (pro) {
		copyFileSync(
			path.join(REPO_ROOT, 'tests', 'fixtures', 'dev-pro.json'),
			path.join(home, 'pro.json')
		);
	}

	const proc = spawn(BIN_PATH, [], {
		env: { ...process.env, WEBLAV_HOME: home },
		stdio: ['ignore', 'pipe', 'pipe']
	});
	let log = '';
	proc.stdout?.on('data', (d) => (log += d));
	proc.stderr?.on('data', (d) => (log += d));

	const stop = async () => {
		liveBackends.delete(stop);
		await killAndWait(proc);
		try {
			// Windows では終了の直後もウイルス対策ソフト等が DB を掴んでいることがあるので、やり直す。
			rmSync(home, { recursive: true, force: true, maxRetries: 5 });
		} catch (err) {
			// 片付けの失敗で、呼び出し側の結果 (テストの成否など) を覆さない。
			console.warn(`一時ディレクトリを消せませんでした (${home}):`, err);
		}
	};

	liveBackends.add(stop);

	// 起動待ちの間は、呼び出し側がまだ片付けを持っていない。Ctrl-C で抜けても残さない。
	// 先に起動した backend も、終了させる前にまとめて片付ける (→ `liveBackends`)。
	const onInterrupt = () => {
		void Promise.all([...liveBackends].map((stopBackend) => stopBackend())).finally(() =>
			process.exit(130)
		);
	};
	process.once('SIGINT', onInterrupt);

	const baseURL = `http://127.0.0.1:${port}`;
	try {
		// db::migrate() は bind より前に実行されるので、health が通れば DB は書き込める。
		await waitForPort(`${baseURL}/api/v1/health`, 10_000, proc);
	} catch (err) {
		console.error('--- backend log ---\n' + log);
		await stop();
		throw err;
	} finally {
		process.off('SIGINT', onInterrupt);
	}
	return { baseURL, dbPath: path.join(home, 'weblav.db'), log: () => log, stop };
}
