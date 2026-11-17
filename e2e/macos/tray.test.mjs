import test from 'node:test';
import assert from 'node:assert/strict';
import {
	PORT,
	launch,
	occupyPort,
	preferredLanguage,
	HOME,
	pressMenuItem,
	pressSubmenuItem,
	quit,
	readMenu,
	readSubmenu,
	waitFor,
	weblavPids
} from '../lib/macos.mjs';
import fs from 'node:fs/promises';
import path from 'node:path';
import { LANGUAGE_NAMES, TRAY_LABELS, labelsFor } from '../lib/tray-labels.mjs';

// メニューバーのアイコンのメニュー (src/tray/)。文言は OS の優先する言語で出る

const titles = (items) => items.map((item) => item.title);

test.describe('macOS: メニューバー', () => {
	let labels;
	let running;

	test.before(async () => {
		labels = labelsFor(await preferredLanguage());
	});

	test.afterEach(async () => {
		await quit(running);
		running = null;
	});

	test('管理者がいないうちは先頭に「セットアップ」が出て、作ると消える。文言は OS の表示言語で出る', async () => {
		running = await launch();
		// 管理者がいるかは起動の後に問い合わせるので、出るまで待つ
		const before = await waitFor(
			() => readMenu(running.pid),
			(items) => items[0]?.title === labels.setup,
			{ label: '「セットアップ」が出る' }
		);
		// 「ログイン時に起動」の文言は、常用の WebLAV と同じ登録の状態で変わる (システム設定で許可が要るとき)
		const shown = titles(before).map((title) =>
			title === labels.loginItemNeedsApproval ? labels.loginItem : title
		);
		assert.deepEqual(shown, [
			labels.setup,
			labels.open,
			labels.manual,
			'',
			labels.language,
			labels.loginItem,
			'',
			labels.quit
		]);

		await running.api.createAdmin();
		await waitFor(
			() => readMenu(running.pid),
			(items) => !titles(items).includes(labels.setup),
			{ label: '管理者を作ると「セットアップ」が消える' }
		);
	});

	test('設定のポートが使われていると次の番号で動き、メニューの先頭に押せない項目でそう出る', async () => {
		const configured = PORT + 10;
		const blocker = await occupyPort(configured);
		try {
			running = await launch({ port: configured });
			assert.equal(running.port, configured + 1);
			const items = await readMenu(running.pid);
			assert.equal(items[0].title, labels.portMoved(configured, configured + 1));
			assert.equal(items[0].enabled, false, 'ポートの知らせは押せない');
			assert.equal(items[1].title, '', '知らせの下に区切りがある');
		} finally {
			blocker.kill();
		}
	});

	test('「表示言語」で選んだ言語にメニューが変わり、config.toml に残る', async () => {
		running = await launch();
		assert.deepEqual(
			(await readSubmenu(running.pid, labels.language)).map(({ title, checked }) => ({ title, checked })),
			[
				{ title: labels.systemLanguage, checked: true },
				{ title: LANGUAGE_NAMES.ja, checked: false },
				{ title: LANGUAGE_NAMES.en, checked: false }
			]
		);
		// OS の表示言語と違うほうを選ぶ
		const other = labels === TRAY_LABELS.ja ? 'en' : 'ja';
		await pressSubmenuItem(running.pid, labels.language, LANGUAGE_NAMES[other]);
		const chosen = TRAY_LABELS[other];
		await waitFor(
			() => readMenu(running.pid),
			(items) => titles(items).includes(chosen.quit),
			{ label: '選んだ言語でメニューが出る' }
		);
		const sub = await readSubmenu(running.pid, chosen.language);
		assert.deepEqual(
			sub.filter((item) => item.checked).map((item) => item.title),
			[LANGUAGE_NAMES[other]]
		);
		await waitFor(
			() => fs.readFile(path.join(HOME, 'config.toml'), 'utf8'),
			(text) => text.includes(`language = "${other}"`) && text.includes('open_setup_on_launch = false'),
			{ label: 'config.toml に選んだ言語が書かれる' }
		);
	});

	test('config.toml で決めた言語で、OS の表示言語によらずメニューが出る', async () => {
		const other = labels === TRAY_LABELS.ja ? 'en' : 'ja';
		running = await launch({ trayLanguage: other });
		const items = await readMenu(running.pid);
		assert.ok(titles(items).includes(TRAY_LABELS[other].quit), JSON.stringify(titles(items)));
	});

	test('「終了する」でプロセスが終わる', async () => {
		running = await launch();
		const { pid } = running;
		await pressMenuItem(pid, labels.quit);
		await waitFor(
			() => weblavPids(),
			(pids) => !pids.includes(pid),
			{ label: '終了する' }
		);
		running = null;
	});
});
