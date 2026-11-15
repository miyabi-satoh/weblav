import test from 'node:test';
import assert from 'node:assert/strict';
import {
	APP,
	launch,
	preferredLanguage,
	pressMenuItem,
	quit,
	readMenu,
	removeFromLoginItems,
	waitFor
} from '../lib/macos.mjs';
import { labelsFor } from '../lib/tray-labels.mjs';

// 「ログイン時に起動」(src/tray/login_item.rs)。登録はアプリの束 (.app) ごとなので、常用の WebLAV の登録を
// 切り替えて元に戻す。/Applications 以外の束で回すと、戻したときに登録がその束を指してしまうので回さない

const REGULAR_APP = '/Applications/WebLAV.app';
const skip = APP !== REGULAR_APP && `${REGULAR_APP} 以外の束 (${APP}) では回さない`;

test.describe('macOS: ログイン時に起動', () => {
	let running;
	let labels;
	const item = async () =>
		(await readMenu(running.pid)).find((m) => m.title.startsWith(labels.loginItem));
	// 起動した直後と押した後は、OS に問い合わせている間押せないので、押せるようになるまで待って読む
	const settled = (ok = () => true) =>
		waitFor(item, (m) => m?.enabled && ok(m), { label: '押せるようになる' });

	/**
	 * 起動して今の登録を読み、`body` を流してから、途中で落ちても常用の WebLAV の登録を元に戻して確かめる。
	 * 戻らなければ、登録が変わったままだと分かるよう、元の失敗と一緒に伝える
	 */
	const keepingRegistration = async (t, body) => {
		labels = labelsFor(await preferredLanguage());
		running = await launch();
		const first = await item();
		assert.ok(first, 'メニューに「ログイン時に起動」がある');
		if (first.title === labels.loginItemNeedsApproval) {
			t.skip('システム設定で許可が要る状態なので、アプリからは切り替えられない');
			return;
		}
		const original = await settled();

		let failure;
		try {
			await body(original);
		} catch (error) {
			failure = error;
		}
		// 動いているほうのメニューは、システム設定で変えた分を読んでいないことがあるので、起動し直して読む
		let restoreError;
		const now = await (async () => {
			await quit(running);
			running = await launch();
			return settled();
		})().catch((error) => {
			restoreError = error;
			return null;
		});
		if (now && now.checked !== original.checked)
			await pressMenuItem(running.pid, labels.loginItem).catch((error) => {
				restoreError = error;
			});
		let restored;
		try {
			restored = await settled((m) => m.checked === original.checked);
		} catch (error) {
			throw new AggregateError(
				[failure, restoreError, error].filter(Boolean),
				'常用の WebLAV の「ログイン時に起動」の登録が、元に戻ったと確かめられない。WebLAV のメニューで確かめて戻す'
			);
		}
		if (failure) throw failure;
		assert.equal(restored.title, labels.loginItem);
	};

	test.afterEach(async () => {
		await quit(running);
	});

	test('押すと OS の登録が切り替わり、もう一度押すと元に戻る', { skip }, (t) =>
		keepingRegistration(t, async (original) => {
			await pressMenuItem(running.pid, labels.loginItem);
			await settled((m) => m.checked === !original.checked);
			await pressMenuItem(running.pid, labels.loginItem);
			await settled((m) => m.checked === original.checked);
		})
	);

	// この macOS では、利用者が止められるのは「ログイン時に開く」の一覧から外すことだけで、外すとオフになる
	// (許可が要る状態にはならない)
	test(
		'システム設定の一覧から外すと、次に起動したときはオフで出て、押すと登録し直す',
		{ skip },
		(t) =>
			keepingRegistration(t, async (original) => {
				if (!original.checked) {
					await pressMenuItem(running.pid, labels.loginItem);
					await settled((m) => m.checked);
				}
				await removeFromLoginItems('WebLAV.app');
				// 動いている間は、メニューバーのアイコンにポインタを載せたときに読み直す。アクセシビリティからは
				// 載せられないので、起動し直して読ませる
				await quit(running);
				running = await launch();
				const off = await settled();
				assert.equal(off.checked, false);
				assert.equal(off.title, labels.loginItem);
				await pressMenuItem(running.pid, labels.loginItem);
				await settled((m) => m.checked);
			})
	);
});
