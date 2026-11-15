import test from 'node:test';
import assert from 'node:assert/strict';
import os from 'node:os';
import { cleanUp, installFresh, launch, quit, removePackage, sleep } from '../lib/windows.mjs';
import { runPowerShell } from '../lib/powershell.mjs';

// 「他の端末からつなぐ」に出す mDNS の名前 (src/mdns.rs)。Windows はコンピューター名を小文字にして `.local` を付ける。
// WebLAV は名乗らず OS の名前を案内するだけなので、起動し直しても名前は変わらず、OS の mDNS で引ける

// mDNS の衝突は、名乗ってから数秒〜数十秒で見つかる
const SETTLE_MS = 30_000;

/** mDNS で引けた IPv4 のアドレス */
async function resolveMdns(name) {
	const stdout = await runPowerShell(
		`ConvertTo-Json -Compress -InputObject @(Resolve-DnsName -Name $args[0] -Type A -ErrorAction SilentlyContinue | Where-Object { $_.IPAddress } | ForEach-Object { $_.IPAddress })`,
		[name]
	);
	return JSON.parse(stdout.trim() || '[]');
}

test.describe('MSIX 版: mDNS の名前', () => {
	let running;

	test.after(async () => {
		await cleanUp([() => quit(running), () => removePackage()]);
	});

	test('案内する名前はコンピューター名に .local を付けたもので、起動し直して30秒たっても変わらず、mDNS で引ける', async () => {
		const expected = `${os.hostname().toLowerCase()}.local`;
		const pkg = await installFresh();
		running = await launch(pkg);
		const first = await running.api.json('GET', '/connection-info');
		assert.equal(first.mdnsHostname, expected);

		running = await launch(pkg, { fresh: false });
		await sleep(SETTLE_MS);
		const again = await running.api.json('GET', '/connection-info');
		assert.equal(again.mdnsHostname, expected);
		assert.ok((await resolveMdns(expected)).length > 0, `${expected} を mDNS で引ける`);
	});
});
