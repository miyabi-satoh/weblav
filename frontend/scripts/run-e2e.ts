// `just e2e-local` (→ `just ci`・CI) から呼ばれる。使い捨ての backend を立てて e2e を流し、片付ける。
//
// 起動済みのサーバーに当てる `just e2e` と違い、開発用 DB を使わない。e2e が作ったものは
// 後片付けするが、途中で落ちたときに開発用 DB を汚さないで済む。
// 管理者は1人だけ入れる。コンテンツは各テストが自分で作る前提 (→ frontend/e2e/api-helpers.ts)。
// 初回セットアップ (管理者が0人のときだけ通る) を試すために、管理者を入れない backend をもう1つ立てる
// (→ frontend/e2e/setup.e2e.ts)。

import { spawn, type ChildProcess, type SpawnOptions } from 'node:child_process';
import path from 'node:path';
import { startBackend, withDatabase, type Backend } from './backend-process.ts';
import { TEST_ADMIN } from './test-account.ts';

/** 中断されたときの終了コード (SIGINT で終わったプロセスの慣習)。 */
const INTERRUPTED_EXIT_CODE = 130;
/** 止める合図を送ってから、強制的に止めて片付けへ進むまで待つ時間。Playwright が後始末を終えるのに足りる長さ。 */
const PLAYWRIGHT_STOP_TIMEOUT_MS = 10_000;

const IS_WINDOWS = process.platform === 'win32';

let interrupted = false;
/** 実行中の Playwright を止める。`runPlaywright` の間だけ入る。 */
let stopRunningPlaywright: (() => void) | undefined;

/**
 * 子と、その下の pnpm・Playwright をまとめて止める。
 * Windows では shell 経由の子が cmd.exe なので、`kill` では下が残る。`taskkill /T` で木ごと止め、
 * それが使えなければ子だけでも止める。それ以外では、子を別のプロセスグループで起動しているので、
 * グループへ送る。
 */
function signalTree(child: ChildProcess, force: boolean): void {
	const pid = child.pid;
	if (pid === undefined) return;
	if (IS_WINDOWS) {
		const killer = spawn('taskkill', ['/pid', String(pid), '/T', '/F'], { stdio: 'ignore' });
		killer.on('error', () => child.kill());
		killer.on('exit', (code) => {
			if (code !== 0) child.kill();
		});
		return;
	}
	try {
		process.kill(-pid, force ? 'SIGKILL' : 'SIGINT');
	} catch {
		// グループが既に無い (終わった後) なら何もしなくてよい。
	}
}

/**
 * Playwright を、使い捨ての backend と管理者で流し、終了コードを返す。中断されたら 130 を返す。
 * `pnpm run test:e2e` を使わないのは、あちらが毎回 `playwright install` (全ブラウザー) を走らせるため。
 * ブラウザーは前もって入れておく (手元では `just install`、CI では workflow の手順で chromium だけを入れる)。
 */
function runPlaywright(
	baseURL: string,
	setupBaseURL: string,
	storageState: string
): Promise<number> {
	return new Promise((resolve, reject) => {
		const options: SpawnOptions = {
			stdio: 'inherit',
			// 別のプロセスグループにして、中断時にグループごと止められるようにする (→ signalTree)。
			// 端末の Ctrl-C は親のグループにしか届かないが、親が受けて転送する。
			detached: !IS_WINDOWS,
			env: {
				...process.env,
				E2E_BASE_URL: baseURL,
				E2E_SETUP_BASE_URL: setupBaseURL,
				E2E_ADMIN_USER: TEST_ADMIN.username,
				E2E_ADMIN_PASSWORD: TEST_ADMIN.password,
				// 手動の `just e2e` とログイン状態のファイルを取り合わないよう、使い捨ての場所に置く。
				E2E_ADMIN_STORAGE_STATE: storageState
			}
		};
		// Windows の pnpm は pnpm.cmd なので、shell を通さないと解決できない。shell を通すときに
		// 引数を配列で渡すと Node が DEP0190 で警告するので、1本のコマンドにする (引数は固定)。
		const child = IS_WINDOWS
			? spawn('pnpm exec playwright test', { ...options, shell: true })
			: spawn('pnpm', ['exec', 'playwright', 'test'], options);

		let settled = false;
		const settle = (result: () => void) => {
			if (settled) return;
			settled = true;
			stopRunningPlaywright = undefined;
			result();
		};
		child.on('error', (err) => settle(() => reject(err)));
		// 中断した後は、子が 0 で終わっても成功にしない。
		child.on('exit', (code) =>
			settle(() => resolve(interrupted ? INTERRUPTED_EXIT_CODE : (code ?? 1)))
		);

		let stopping = false;
		stopRunningPlaywright = () => {
			if (stopping) return;
			stopping = true;
			signalTree(child, false);
			// 合図を無視されても、強制的に止めて片付けへ進む。子が残っても待ち続けない。
			setTimeout(() => {
				signalTree(child, true);
				settle(() => resolve(INTERRUPTED_EXIT_CODE));
			}, PLAYWRIGHT_STOP_TIMEOUT_MS).unref();
		};
		// spawn の直前に中断されていたら、起動した直後に止める。
		if (interrupted) stopRunningPlaywright();
	});
}

async function main(): Promise<number> {
	// 親まで落ちると finally の片付けが走らず、一時 WEBLAV_HOME が残る。親は落とさずに、
	// Playwright を止めてから片付けへ進める。何度受けても止める処理は1回だけ。
	process.on('SIGINT', () => {
		interrupted = true;
		stopRunningPlaywright?.();
	});

	const backend = await startBackend('weblav-e2e-');
	let setupBackend: Backend | undefined;
	try {
		setupBackend = await startBackend('weblav-e2e-setup-');
		withDatabase(backend.dbPath, (db) =>
			db
				.prepare(
					"INSERT INTO users (username, password_hash, role, recovery_code_hash) VALUES (?, ?, 'admin', ?)"
				)
				.run(TEST_ADMIN.username, TEST_ADMIN.passwordHash, TEST_ADMIN.recoveryCodeHash)
		);
		// 準備の途中で中断されたら、テストを始めずに片付ける。
		if (interrupted) return INTERRUPTED_EXIT_CODE;
		const storageState = path.join(path.dirname(backend.dbPath), 'e2e-admin.json');
		return await runPlaywright(backend.baseURL, setupBackend.baseURL, storageState);
	} finally {
		await setupBackend?.stop();
		await backend.stop();
	}
}

main().then(
	(code) => {
		process.exitCode = code;
	},
	(err) => {
		console.error(err);
		process.exitCode = 1;
	}
);
