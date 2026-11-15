import test from 'node:test';
import assert from 'node:assert/strict';
import {
	APP,
	launch,
	preferredLanguage,
	pressMenuItem,
	quit,
	readMenu,
	waitFor
} from '../lib/macos.mjs';
import { labelsFor } from '../lib/tray-labels.mjs';

// 「ログイン時に起動」(src/tray/login_item.rs)。登録はアプリの束 (.app) ごとなので、常用の WebLAV の登録を
// 切り替えて元に戻す。/Applications 以外の束で回すと、戻したときに登録がその束を指してしまうので回さない

const REGULAR_APP = '/Applications/WebLAV.app';

test.describe('macOS: ログイン時に起動', () => {
	let running;

	test.after(async () => {
		await quit(running);
	});

	test(
		'押すと OS の登録が切り替わり、もう一度押すと元に戻る',
		{ skip: APP !== REGULAR_APP && `${REGULAR_APP} 以外の束 (${APP}) では回さない` },
		async (t) => {
			const labels = labelsFor(await preferredLanguage());
			running = await launch();
			const item = async () =>
				(await readMenu(running.pid)).find((m) => m.title.startsWith(labels.loginItem));
			const first = await item();
			assert.ok(first, 'メニューに「ログイン時に起動」がある');
			if (first.title === labels.loginItemNeedsApproval) {
				t.skip('システム設定で許可が要る状態なので、アプリからは切り替えられない');
				return;
			}
			// 起動した直後は OS に問い合わせている間押せないので、押せるようになるまで待つ
			const original = await waitFor(item, (m) => m?.enabled, { label: '押せるようになる' });

			let failure;
			try {
				await pressMenuItem(running.pid, labels.loginItem);
				await waitFor(item, (m) => m?.enabled && m.checked === !original.checked, {
					label: '押すと切り替わる'
				});
				await pressMenuItem(running.pid, labels.loginItem);
				await waitFor(item, (m) => m?.enabled && m.checked === original.checked, {
					label: 'もう一度押すと戻る'
				});
			} catch (error) {
				failure = error;
			}
			// 途中で落ちても、常用の WebLAV の登録を元に戻し、戻ったことを確かめる。OS への問い合わせの間は押せないので、
			// 押せるのを待つ。戻らなければ、登録が変わったままだと分かるよう、元の失敗と一緒に伝える
			const now = await waitFor(item, (m) => m?.enabled).catch(() => null);
			if (now && now.checked !== original.checked)
				await pressMenuItem(running.pid, labels.loginItem).catch(() => {});
			const restored = await waitFor(item, (m) => m?.enabled && m.checked === original.checked, {
				label: '元の登録に戻っている'
			}).catch((error) => {
				throw failure
					? new AggregateError([failure, error], '常用の WebLAV の登録が戻っていない')
					: error;
			});
			if (failure) throw failure;
			assert.equal(restored.title, labels.loginItem);
		}
	);
});
