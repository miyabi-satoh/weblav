// macOS と Windows の確認で共通の部品 (待ち・後片付け・確かめ用の WebLAV の API)。
import fs from 'node:fs/promises';

export const PASSWORD = 'e2e-password-1';

export async function waitFor(read, ok, { label, timeout = 10_000, interval = 200 } = {}) {
	const deadline = Date.now() + timeout;
	let last;
	for (;;) {
		last = await read();
		if (ok(last)) return last;
		if (Date.now() > deadline)
			throw new Error(
				`${label ?? '待ち'}: ${timeout}ms たっても満たされない (最後の値: ${JSON.stringify(last)})`
			);
		await new Promise((resolve) => setTimeout(resolve, interval));
	}
}

export const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

/** 後片付けの手順を順に流す。1つが落ちても残りを片付け、落ちたものは出す */
export async function cleanUp(steps) {
	for (const step of steps) {
		try {
			await step();
		} catch (error) {
			console.error(error);
		}
	}
}

/** ファイルやフォルダーが無いことを確かめる */
export async function assertMissing(file, message) {
	const exists = await fs.access(file).then(
		() => true,
		() => false
	);
	if (exists) throw new Error(message);
}

/** 共有の中の一覧 (`Api.browse`) の名前を、並べて返す */
export const entryNames = (browse) => browse.entries.map((entry) => entry.name).sort();

/** クッキーを持ち回る、確かめ用の WebLAV の API */
export class Api {
	constructor(port) {
		this.port = port;
		this.origin = `http://localhost:${port}`;
		this.cookie = '';
	}

	async request(method, url, { json, form } = {}) {
		const headers = { origin: this.origin };
		if (this.cookie) headers.cookie = this.cookie;
		let body;
		if (json !== undefined) {
			headers['content-type'] = 'application/json';
			body = JSON.stringify(json);
		} else if (form) body = form;
		const response = await fetch(`${this.origin}/api/v1${url}`, { method, headers, body });
		const set = response.headers.getSetCookie?.() ?? [];
		if (set.length > 0) this.cookie = set.map((c) => c.split(';')[0]).join('; ');
		return response;
	}

	async status(method, url, options) {
		return (await this.request(method, url, options)).status;
	}

	async json(method, url, options) {
		const response = await this.request(method, url, options);
		const text = await response.text();
		if (!response.ok) throw new Error(`${method} ${url}: ${response.status} ${text}`);
		return text ? JSON.parse(text) : null;
	}

	/** 最初の管理者を作り、ログインした状態にする */
	async createAdmin() {
		const token = await (await this.request('POST', '/setup/token')).text();
		await this.json('POST', '/setup/admin', {
			json: { token, username: 'admin', password: PASSWORD }
		});
	}

	login() {
		return this.json('POST', '/auth/login', { json: { username: 'admin', password: PASSWORD } });
	}

	/** フォルダー選択の窓を出させる。窓が閉じると、選ばれたパス (取り消しは null) で解決する */
	pick() {
		return this.json('POST', '/admin/roots/pick').then((body) => body.path);
	}

	addRoot(folderPath) {
		return this.json('POST', '/admin/roots', { json: { path: folderPath } });
	}

	browse(shareId, relative = '') {
		return this.json('GET', `/shares/${shareId}/browse?path=${encodeURIComponent(relative)}`);
	}

	/** 処理 (削除・移動など) が終わるまで待ち、終わったときの進み具合を返す */
	waitJob(jobId) {
		return waitFor(
			() => this.json('GET', `/shares/jobs/${jobId}`),
			(job) => job.state !== 'running',
			{ label: '処理が終わる', timeout: 30_000 }
		);
	}
}
