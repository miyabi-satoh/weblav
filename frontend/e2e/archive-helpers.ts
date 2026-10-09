// アーカイブの管理画面 (軸・アイテム) を操作する共通の知識。
import { expect, type Locator, type Page } from '@playwright/test';
import { escapeRegExp, requiredLabel, SAVE_BUTTON_NAME, saveForm, waitForDialog } from './helpers';

/** 軸の画面の「軸を追加...」。admin にだけ出る。 */
export const ADD_AXIS_BUTTON_NAME = /^軸を追加\.\.\.$|^Add axis\.\.\.$/;

// ラベルは文言の前後で改行しているので、前後の空白を許す。
const SOURCE_LABEL = /^\s*(抽出元|Source)\s*$/;

/** 軸の追加・編集ダイアログで、フォルダーの階層を選ぶラジオ (「第N階層」)。 */
export function dirLevelRadioLabel(level: number): RegExp {
	return new RegExp(`^\\s*(第${level}階層|Level ${level})\\s*$`);
}

/**
 * `name` の軸の値の辞書が開き、読み込みを終えるまで待つ。
 * 画面は読み込み中の保存ボタンを押せなくするので、見出しがその軸のもので、保存ボタンが押せれば読み込み済み。
 * 待たずに行を足すと、届いた応答で表が置き換わって足した行が消える。
 */
async function waitForAxisValuesLoaded(page: Page, name: string) {
	const label = new RegExp(`^${escapeRegExp(name)} の値$|^Values for ${escapeRegExp(name)}$`);
	await expect(page.getByRole('heading', { name: label })).toBeVisible();
	await expect(axisValuesForm(page).getByRole('button', { name: SAVE_BUTTON_NAME })).toBeEnabled();
}

/**
 * 軸の追加ダイアログを開いて、名前と抽出元を入れて保存する。
 * フォルダーの階層は、スキャン済みのアイテムの階層ごとの選択肢から選ぶ (アイテムがある前提)。
 */
export async function addAxis(
	page: Page,
	name: string,
	source: { dirLevel: number } | { filenameWord: true }
) {
	await page.getByRole('button', { name: ADD_AXIS_BUTTON_NAME }).click();
	await waitForDialog(page);
	const dialog = page.getByRole('dialog');
	await dialog.getByLabel(requiredLabel('軸名', 'Axis name')).fill(name);
	await dialog.getByLabel(SOURCE_LABEL).click();
	if ('dirLevel' in source) {
		await page.getByRole('option', { name: /^フォルダーの階層$|^Folder level$/ }).click();
		await dialog.getByLabel(dirLevelRadioLabel(source.dirLevel)).check();
	} else {
		await page
			.getByRole('option', { name: /^ファイル名に含まれる語$|^Word in file name$/ })
			.click();
	}
	await dialog.getByRole('button', { name: SAVE_BUTTON_NAME }).click();
	await expect(page.getByRole('dialog')).toHaveCount(0);
	// 追加した軸は選ばれた状態になり、その値の辞書を読み込む。
	await waitForAxisValuesLoaded(page, name);
}

/** 軸の一覧から軸を選んで値の辞書を開き、読み込みを待つ。 */
export async function selectAxis(page: Page, name: string) {
	await page
		.getByRole('button')
		.filter({ has: page.getByText(name, { exact: true }) })
		.click();
	await waitForAxisValuesLoaded(page, name);
}

/** 値の辞書を囲むフォーム。見出し (「<軸名> の値」) で名前が付いている。 */
export function axisValuesForm(page: Page): Locator {
	return page.getByRole('form', { name: /の値$|^Values for / });
}

/** 値の辞書に行を足し、元の値と表示名を入れる (保存はしない)。 */
export async function addAxisValue(page: Page, raw: string, display: string) {
	await page.getByRole('button', { name: /^値を追加する$|^Add value$/ }).click();
	const row = axisValuesForm(page).locator('tbody tr').last();
	await row.getByRole('textbox').nth(0).fill(raw);
	await row.getByRole('textbox').nth(1).fill(display);
}

/** 値の辞書を保存し、その軸の保存を知らせるトーストを待つ。 */
export async function saveAxisValues(page: Page, axisName: string) {
	await saveForm(page, {
		button: axisValuesForm(page).getByRole('button', { name: SAVE_BUTTON_NAME }),
		toastText: new RegExp(
			`${escapeRegExp(axisName)} の値を保存しました|Values for ${escapeRegExp(axisName)} saved\\.`
		)
	});
}

/** アイテムの一覧で、パスが `relPath` の行の公開スイッチ。 */
export function itemPublishedSwitch(page: Page, relPath: string): Locator {
	return page
		.getByRole('row')
		.filter({ hasText: relPath })
		.getByRole('switch', { name: /を公開$|^Publish / });
}

/** 値の辞書の「ファイル名によく出る語」の区画。 */
export function wordCandidatesRegion(page: Page): Locator {
	return page.getByRole('region', {
		name: /^ファイル名によく出る語$|^Common words in file names$/
	});
}

/** 語の候補の1つ。名前は語と件数 (「listening 2」)。 */
export function wordCandidate(page: Page, word: string, count: number): Locator {
	return wordCandidatesRegion(page).getByRole('button', {
		name: new RegExp(`^${escapeRegExp(word)}\\s*${count}$`)
	});
}

/** 値の辞書の表の下に出る、保存前の表での一致・未設定の件数。表が空だと表の見出しが無いので、画面全体から探す。 */
export function axisValuesPreviewSummary(page: Page, matched: number, unset: number): Locator {
	return page.getByText(
		new RegExp(`^一致 ${matched}件 / 未設定 ${unset}件$|^Matched: ${matched} / Unset: ${unset}$`)
	);
}
