import test from 'node:test';
import assert from 'node:assert/strict';
import { launch, localHostName, quit, sleep } from '../lib/macos.mjs';

// 「他の端末からつなぐ」に出す mDNS の名前 (src/mdns.rs)。WebLAV は名乗らず OS の名前を案内するだけなので、
// 起動し直しても OS が衝突とみなして名前を付け直さない

// mDNS の衝突は、名乗ってから数秒〜数十秒で見つかる
const SETTLE_MS = 30_000;

test.describe('macOS: mDNS の名前', () => {
	let running;

	test.after(async () => {
		await quit(running);
	});

	test('案内する名前は OS のローカルホスト名で、起動し直して30秒たっても変わらない', async () => {
		const before = await localHostName();
		running = await launch();
		const first = await running.api.json('GET', '/connection-info');
		assert.equal(first.mdnsHostname, `${before}.local`);

		await quit(running);
		running = await launch({ fresh: false });
		await sleep(SETTLE_MS);
		assert.equal(await localHostName(), before, 'OS のローカルホスト名が付け直されていない');
		const again = await running.api.json('GET', '/connection-info');
		assert.equal(again.mdnsHostname, `${before}.local`);
	});
});
