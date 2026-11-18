import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import path from 'node:path';
import {
	HOME,
	PORT,
	launch,
	occupyPort,
	pressMenuItem,
	pressSubmenuItem,
	quit,
	readMenu,
	readSubmenu,
	waitFor,
	weblavPids
} from '../lib/linux.mjs';
import { LANGUAGE_NAMES, TRAY_LABELS } from '../lib/tray-labels.mjs';

// トレイのアイコンのメニュー (src/tray/)。文言は、ロケールの環境変数 (LANG) で出る (src/tray/mod.rs の tray_locale)

const titles = (items) => items.map((item) => item.title);

test.describe('Linux: トレイ', () => {
	let running;

	test.afterEach(async () => {
		await quit(running);
		running = null;
	});

	for (const [locale, labels] of [
		['ja_JP.UTF-8', TRAY_LABELS.ja],
		['en_US.UTF-8', TRAY_LABELS.en]
	]) {
		test(`LANG=${locale} では、管理者がいないうちは先頭に「セットアップ」が出て、作ると消える`, async () => {
			running = await launch({ locale });
			// 管理者がいるかは起動の後に問い合わせるので、出るまで待つ
			const before = await waitFor(
				() => readMenu(running.pid),
				(items) => items[0]?.title === labels.setup,
				{ label: '「セットアップ」が出る' }
			);
			assert.deepEqual(titles(before), [
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
	}

	test('設定のポートが使われていると次の番号で動き、メニューの先頭に押せない項目でそう出る', async () => {
		const configured = PORT + 10;
		const blocker = await occupyPort(configured);
		try {
			running = await launch({ port: configured });
			assert.equal(running.port, configured + 1);
			const items = await readMenu(running.pid);
			assert.equal(items[0].title, TRAY_LABELS.ja.portMoved(configured, configured + 1));
			assert.equal(items[0].enabled, false, 'ポートの知らせは押せない');
			assert.equal(items[1].title, '', '知らせの下に区切りがある');
		} finally {
			blocker.kill();
		}
	});

	test('「表示言語」で選んだ言語にメニューが変わり、config.toml に残る', async () => {
		const labels = TRAY_LABELS.ja;
		running = await launch();
		assert.deepEqual(
			(await readSubmenu(running.pid, labels.language)).map(({ title, checked }) => ({
				title,
				checked
			})),
			[
				{ title: labels.systemLanguage, checked: true },
				{ title: LANGUAGE_NAMES.ja, checked: false },
				{ title: LANGUAGE_NAMES.en, checked: false }
			]
		);
		await pressSubmenuItem(running.pid, labels.language, LANGUAGE_NAMES.en);
		const chosen = TRAY_LABELS.en;
		await waitFor(
			() => readMenu(running.pid),
			(items) => titles(items).includes(chosen.quit),
			{ label: '選んだ言語でメニューが出る' }
		);
		const sub = await readSubmenu(running.pid, chosen.language);
		assert.deepEqual(
			sub.filter((item) => item.checked).map((item) => item.title),
			[LANGUAGE_NAMES.en]
		);
		await waitFor(
			() => fs.readFile(path.join(HOME, 'config.toml'), 'utf8'),
			(text) => text.includes('language = "en"') && text.includes('open_setup_on_launch = false'),
			{ label: 'config.toml に選んだ言語が書かれる' }
		);
	});

	test('config.toml で決めた言語で、LANG によらずメニューが出る', async () => {
		running = await launch({ locale: 'ja_JP.UTF-8', trayLanguage: 'en' });
		const items = await readMenu(running.pid);
		assert.ok(titles(items).includes(TRAY_LABELS.en.quit), JSON.stringify(titles(items)));
	});

	test('「終了する」でプロセスが終わる', async () => {
		running = await launch();
		const { pid } = running;
		await pressMenuItem(pid, TRAY_LABELS.ja.quit);
		await waitFor(
			() => weblavPids(),
			(pids) => !pids.includes(pid),
			{ label: '終了する' }
		);
		running = null;
	});
});
