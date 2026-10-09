// フォルダー・アーカイブの画面 (管理者ログイン済み)。
// ファイルシステムの実体が要るので、テストごとに target/ 配下へ小さな木を作り、API で登録する
// (→ fixture-helpers.ts の withDirectoryContent)。サーバーからそのパスが見えなければテストを飛ばす。
import { test, expect, type Page } from '@playwright/test';
import {
	AUDIO_PLAYER_REGION_NAME,
	AUTO_ADVANCE_BUTTON_NAME,
	CANCEL_BUTTON_NAME,
	CLOSE_BUTTON_NAME,
	confirmDelete,
	ITEMS_TAB_NAME,
	NEXT_FILE_BUTTON_NAME,
	NEXT_TRACK_BUTTON_NAME,
	PREVIOUS_TRACK_BUTTON_NAME,
	requiredLabel,
	SAVE_BUTTON_NAME,
	saveForm,
	waitForDialog
} from './helpers';
import { send } from './api-helpers';
import { rescanArchive, setUpArchiveForBrowsing, withDirectoryContent } from './fixture-helpers';
import {
	ADD_AXIS_BUTTON_NAME,
	addAxis,
	addAxisValue,
	dirLevelRadioLabel,
	axisValuesForm,
	axisValuesPreviewSummary,
	itemPublishedSwitch,
	saveAxisValues,
	selectAxis,
	wordCandidate,
	wordCandidatesRegion
} from './archive-helpers';

// 操作が見つからないまま待ち続けるとテストの制限時間を使い切り、finally の後片付け
// (API での削除) まで時間切れになってコンテンツと木が残る。操作ごとの待ちを短くして、
// 失敗しても片付ける時間を残す。
test.use({ actionTimeout: 10_000, navigationTimeout: 10_000 });

// ラベルは文言の前後で改行しているので、前後の空白を許す。
const TITLE_TEMPLATE_LABEL = /^\s*(表示タイトルのテンプレート|Title template)\s*$/;

test('軸: 軸を足して値の辞書と表示タイトルを保存すると、アイテムのタイトルが組み立つ', async ({
	page
}) => {
	await withDirectoryContent(
		page.request,
		{ type: 'archive', namePrefix: 'e2e-axes' },
		async (archive) => {
			await rescanArchive(page.request, archive.id);

			await page.goto(`/admin/contents/${archive.id}/axes`);
			await addAxis(page, '年度', { dirLevel: 1 });
			await addAxis(page, '種類', { filenameWord: true });

			await addAxisValue(page, 'listening', 'リスニング');
			await addAxisValue(page, 'answer', '解答');
			await saveAxisValues(page, '種類');

			// `getByLabel` はフォームの名前 (見出し) にも当たるので、入力欄は role で取る。
			await page.getByRole('textbox', { name: TITLE_TEMPLATE_LABEL }).fill('{年度} {種類}');
			// 画面に保存ボタンが2つあるので、見出しで名前の付いたフォームに絞る。
			const templateForm = page.getByRole('form', { name: TITLE_TEMPLATE_LABEL });
			await saveForm(page, {
				button: templateForm.getByRole('button', { name: SAVE_BUTTON_NAME }),
				toastText: /表示タイトルのテンプレートを保存しました|Title template saved\./
			});

			await page.getByRole('link', { name: ITEMS_TAB_NAME }).click();
			await expect(page.getByRole('cell', { name: '2024 リスニング' })).toBeVisible();
			await expect(page.getByRole('cell', { name: '2024 解答' })).toBeVisible();
			await expect(page.getByRole('cell', { name: '2023 リスニング' })).toBeVisible();
		}
	);
});

test('軸の追加: フォルダーの階層は、階層ごとの値の例を見て選べる', async ({ page }) => {
	await withDirectoryContent(
		page.request,
		{
			type: 'archive',
			namePrefix: 'e2e-axis-dir-levels',
			files: {
				'2024/1st/listening.mp3': 'ID3',
				'2024/2nd/listening.mp3': 'ID3',
				'2023/1st/answer.pdf': '%PDF-1.4\n%%EOF\n'
			}
		},
		async (archive) => {
			await rescanArchive(page.request, archive.id);

			await page.goto(`/admin/contents/${archive.id}/axes`);
			await page.getByRole('button', { name: ADD_AXIS_BUTTON_NAME }).click();
			await waitForDialog(page);
			const dialog = page.getByRole('dialog');
			await dialog.getByLabel(requiredLabel('軸名', 'Axis name')).fill('回');

			// 新規の抽出元はフォルダーの階層で、第1階層が選ばれている。値の例は件数の多い順。
			const level1 = dialog.getByLabel(dirLevelRadioLabel(1));
			const level2 = dialog.getByLabel(dirLevelRadioLabel(2));
			await expect(level1).toBeChecked();
			await expect(level1).toHaveAccessibleDescription(
				/^2024・2023 \(2種類\)$|^2024, 2023 \(distinct values: 2\)$/
			);
			await expect(level2).toHaveAccessibleDescription(
				/^1st・2nd \(2種類\)$|^1st, 2nd \(distinct values: 2\)$/
			);

			await level2.check();
			await expect(level2).toBeChecked();
			await dialog.getByRole('button', { name: SAVE_BUTTON_NAME }).click();
			await expect(page.getByRole('dialog')).toHaveCount(0);

			await expect(axisRow(page, '回')).toContainText(/フォルダーの第2階層|Folder level 2/);
		}
	);
});

test('アイテム: 再スキャンで索引し、公開を切り替えられる', async ({ page }) => {
	await withDirectoryContent(
		page.request,
		{ type: 'archive', namePrefix: 'e2e-items' },
		async (archive) => {
			await page.goto(`/admin/contents/${archive.id}/items`);
			await page.getByRole('button', { name: /^再スキャンする$|^Rescan$/ }).click();
			await expect(page.getByText(/新規3件・削除0件|3 added, 0 removed/)).toBeVisible();
			// 新しく索引したアイテムは非公開から始まる。
			await expect(page.getByText(/3\s件\(公開 0\)|3 items \(0 published\)/)).toBeVisible();

			const toggle = itemPublishedSwitch(page, '2024/listening.mp3');
			await expect(toggle).toHaveAttribute('aria-checked', 'false');
			await toggle.click();
			await expect(toggle).toHaveAttribute('aria-checked', 'true');
			await expect(page.getByText(/3\s件\(公開 1\)|3 items \(1 published\)/)).toBeVisible();
		}
	);
});

test('アーカイブの閲覧: 軸で絞り込み、音声を押すとページ内のプレイヤーで開く', async ({ page }) => {
	await withDirectoryContent(
		page.request,
		{ type: 'archive', namePrefix: 'e2e-archive-view' },
		async (archive, name) => {
			await setUpArchiveForBrowsing(page.request, archive);

			await page.goto(`/archives/${archive.id}`);
			await expect(page.getByRole('heading', { level: 1, name })).toBeVisible();
			await expect(page.getByText(/^3\s件$|^3 items$/)).toBeVisible();
			// 音声はページ内で再生するボタン、PDF は新規タブで開くリンク。
			await expect(page.getByRole('button', { name: /2024 リスニング/ })).toBeVisible();
			await expect(page.getByRole('link', { name: /2024 解答/ })).toHaveAttribute(
				'target',
				'_blank'
			);

			// 絞り込みの開くボタンは、軸名を aria-label に持つ。
			await page.getByRole('button', { name: '年度', exact: true }).click();
			await page.getByRole('option', { name: /^2023/ }).click();
			await page.waitForURL((url) => url.searchParams.get('年度') === '2023');
			await expect(page.getByText(/^1\s件$|^1 items$/)).toBeVisible();
			await expect(page.getByRole('button', { name: /2024 リスニング/ })).toHaveCount(0);

			await page.getByRole('button', { name: /2023 リスニング/ }).click();
			const player = page.getByRole('region', { name: AUDIO_PLAYER_REGION_NAME });
			await expect(player).toBeVisible();
			await expect(page.locator('audio')).toHaveAttribute(
				'src',
				new RegExp(`/api/v1/contents/${archive.id}/items/\\d+/download$`)
			);
			// 閉じるとプレイヤーが消える。
			await player.getByRole('button', { name: CLOSE_BUTTON_NAME }).click();
			await expect(player).toHaveCount(0);
		}
	);
});

test('プレイヤーの前・次の曲: 一覧に並んだ音声の順に移り、端では次へ進めない', async ({ page }) => {
	await withDirectoryContent(
		page.request,
		{ type: 'archive', namePrefix: 'e2e-archive-queue' },
		async (archive, name) => {
			await setUpArchiveForBrowsing(page.request, archive);

			await page.goto(`/archives/${archive.id}`);
			await expect(page.getByRole('heading', { level: 1, name })).toBeVisible();
			// 並び順はサーバーが決めるので、画面に並んだ順を正とする。PDF の行は前後に数えない。
			const audioRows = page.getByRole('button', { name: /リスニング/ });
			await expect(audioRows).toHaveCount(2);
			const [first, second] = await audioRows.allInnerTexts();

			await audioRows.first().click();
			const player = page.getByRole('region', { name: AUDIO_PLAYER_REGION_NAME });
			await expect(player).toContainText(first.split('\n')[0]);
			const next = player.getByRole('button', { name: NEXT_TRACK_BUTTON_NAME });
			await expect(next).toBeEnabled();

			await next.click();
			await expect(player).toContainText(second.split('\n')[0]);
			await expect(next).toBeDisabled();

			// 再生位置が頭なので、「前の曲」は前の曲へ移る。
			await player.getByRole('button', { name: PREVIOUS_TRACK_BUTTON_NAME }).click();
			await expect(player).toContainText(first.split('\n')[0]);
			await expect(next).toBeEnabled();
		}
	);
});

test('プレイヤーの続けて再生: 切り替えた状態を、読み込み直しても覚えている', async ({ page }) => {
	await withDirectoryContent(
		page.request,
		{ type: 'archive', namePrefix: 'e2e-archive-auto' },
		async (archive) => {
			await setUpArchiveForBrowsing(page.request, archive);
			const player = page.getByRole('region', { name: AUDIO_PLAYER_REGION_NAME });
			const autoAdvance = player.getByRole('button', { name: AUTO_ADVANCE_BUTTON_NAME });

			await page.goto(`/archives/${archive.id}`);
			await page
				.getByRole('button', { name: /リスニング/ })
				.first()
				.click();
			await expect(autoAdvance).toHaveAttribute('aria-pressed', 'false');
			await autoAdvance.click();
			await expect(autoAdvance).toHaveAttribute('aria-pressed', 'true');

			await page.reload();
			await page
				.getByRole('button', { name: /リスニング/ })
				.first()
				.click();
			await expect(autoAdvance).toHaveAttribute('aria-pressed', 'true');
		}
	);
});

test('フォルダーの閲覧: 下の階層へ移り、パンくずで戻れる', async ({ page }) => {
	await withDirectoryContent(
		page.request,
		{ type: 'folder', namePrefix: 'e2e-folder' },
		async (folder, name) => {
			await page.goto(`/folders/${folder.id}`);
			await expect(page.getByRole('heading', { level: 1, name })).toBeVisible();
			await expect(page.getByRole('link', { name: '2023', exact: true })).toBeVisible();

			await page.getByRole('link', { name: '2024', exact: true }).click();
			await page.waitForURL((url) => url.searchParams.get('path') === '2024');
			await expect(page.getByRole('heading', { level: 1, name: '2024' })).toBeVisible();
			await expect(page.getByRole('button', { name: /listening\.mp3/ })).toBeVisible();
			await expect(page.getByRole('link', { name: /answer\.pdf/ })).toHaveAttribute(
				'target',
				'_blank'
			);

			await page.getByRole('link', { name, exact: true }).click();
			await page.waitForURL(
				(url) => url.pathname === `/folders/${folder.id}` && !url.searchParams.has('path')
			);
			await expect(page.getByRole('heading', { level: 1, name })).toBeVisible();
		}
	);
});

test('ファイルのビューアー: テキストと PDF をページ内で開き、前後に移れる', async ({ page }) => {
	await withDirectoryContent(
		page.request,
		{
			type: 'folder',
			namePrefix: 'e2e-viewer',
			// テキストは拡張子でなく中身で決まるので、.txt でないファイルで確かめる。
			// PDF は中身が PDF でないので読めず、ビューアーは新しいタブで開く案内に替わる。
			files: {
				'a-config.yml': 'ビューアーの本文',
				'b-answer.pdf': 'not a pdf',
				'c-listening.mp3': 'ID3'
			}
		},
		async (folder) => {
			await page.goto(`/folders/${folder.id}`);
			await page.getByRole('link', { name: /a-config\.yml/ }).click();
			const viewer = page.getByRole('dialog', { name: 'a-config.yml' });
			await expect(viewer.getByText('ビューアーの本文')).toBeVisible();
			await expect(viewer.getByText('1 / 2')).toBeVisible();

			// 音声の行は前後に入らない。
			await viewer.getByRole('button', { name: NEXT_FILE_BUTTON_NAME }).click();
			const pdfViewer = page.getByRole('dialog', { name: 'b-answer.pdf' });
			await expect(pdfViewer.getByText('2 / 2')).toBeVisible();
			await expect(
				pdfViewer.getByRole('link', { name: /新しいタブで開く|Opens in a new tab/ }).last()
			).toHaveAttribute('href', /\/download/);
			await expect(pdfViewer.getByRole('button', { name: NEXT_FILE_BUTTON_NAME })).toBeDisabled();

			await page.keyboard.press('Escape');
			await expect(pdfViewer).toBeHidden();
		}
	);
});

// ここから下は操作の細部。

/**
 * 軸の一覧の行。行に役割が無いので、軸名を名前に持つ「上へ」ボタンの親を取る
 * (上下ボタンは lg 以上だけ出る。e2e は Desktop Chrome の幅で流す)。
 */
function axisRow(page: Page, name: string) {
	return page
		.getByRole('button', { name: new RegExp(`^${name} を上へ$|^Move ${name} up$`) })
		.locator('xpath=..');
}

/** 閲覧画面の絞り込みの開くボタン (軸名を aria-label に持つ)。 */
function filterTriggers(page: Page, names: string[]) {
	return page.getByRole('button', { name: new RegExp(`^(${names.join('|')})$`) });
}

test('軸の並べ替え: 上下ボタンで動かした順が保存され、閲覧の絞り込みの並びに出る', async ({
	page
}) => {
	await withDirectoryContent(
		page.request,
		{ type: 'archive', namePrefix: 'e2e-axis-order' },
		async (archive) => {
			await setUpArchiveForBrowsing(page.request, archive, { kindFilterable: true });

			// 動かす前は登録した順 (年度・種類)。
			await page.goto(`/archives/${archive.id}`);
			const triggers = filterTriggers(page, ['年度', '種類']);
			await expect(triggers).toHaveCount(2);
			await expect(triggers.nth(0)).toHaveAttribute('aria-label', '年度');

			await page.goto(`/admin/contents/${archive.id}/axes`);
			await axisRow(page, '種類')
				.getByRole('button', { name: /^種類 を上へ$|^Move 種類 up$/ })
				.click();
			// 保存中は上下ボタンがすべて止まる。2番目になった年度の「上へ」が押せるようになれば保存を終えている。
			await expect(
				page.getByRole('button', { name: /^年度 を上へ$|^Move 年度 up$/ })
			).toBeEnabled();

			await page.goto(`/archives/${archive.id}`);
			await expect(triggers).toHaveCount(2);
			await expect(triggers.nth(0)).toHaveAttribute('aria-label', '種類');
			await expect(triggers.nth(1)).toHaveAttribute('aria-label', '年度');
		}
	);
});

test('値の辞書: 行を上へ動かして保存するとアイテムの並びが変わり、行を削除して保存すると値が外れる', async ({
	page
}) => {
	await withDirectoryContent(
		page.request,
		{ type: 'archive', namePrefix: 'e2e-axis-values' },
		async (archive) => {
			await setUpArchiveForBrowsing(page.request, archive);

			// 並びは軸の順 (年度→種類)、種類の中は辞書の行の順 (listening→answer)。
			await page.goto(`/archives/${archive.id}`);
			const items = page.getByRole('listitem');
			await expect(items).toHaveText([/2023 リスニング/, /2024 リスニング/, /2024 解答/]);

			await page.goto(`/admin/contents/${archive.id}/axes`);
			await selectAxis(page, '種類');
			const rows = axisValuesForm(page).locator('tbody tr');
			await expect(rows).toHaveCount(2);

			await page.getByRole('button', { name: /^answer を上へ$|^Move answer up$/ }).click();
			await expect(rows.nth(0).getByRole('textbox').nth(0)).toHaveValue('answer');
			await saveAxisValues(page, '種類');

			await page.goto(`/archives/${archive.id}`);
			await expect(items).toHaveText([/2023 リスニング/, /2024 解答/, /2024 リスニング/]);

			await page.goto(`/admin/contents/${archive.id}/axes`);
			await selectAxis(page, '種類');
			// 入力欄の値は属性に出ないので行を値で絞れない。保存した順 (answer→listening) の2行目を取る。
			await expect(rows).toHaveCount(2);
			const listeningRow = rows.nth(1);
			await expect(listeningRow.getByRole('textbox').nth(0)).toHaveValue('listening');
			await listeningRow
				.getByRole('button', { name: /^listening を削除する$|^Remove listening$/ })
				.click();
			await expect(rows).toHaveCount(1);
			await saveAxisValues(page, '種類');

			// listening はもう照合されないので、その値の表示名は出ない。残した answer は出る (対照)。
			await page.goto(`/archives/${archive.id}`);
			await expect(page.getByRole('link', { name: /2024 解答/ })).toBeVisible();
			await expect(page.getByText(/リスニング/)).toHaveCount(0);
		}
	);
});

test('値の辞書: ファイル名の語の候補を押すと表に行が足され、一致する件数が変わる', async ({
	page
}) => {
	await withDirectoryContent(
		page.request,
		{ type: 'archive', namePrefix: 'e2e-axis-words' },
		async (archive) => {
			await rescanArchive(page.request, archive.id);

			await page.goto(`/admin/contents/${archive.id}/axes`);
			await addAxis(page, '種類', { filenameWord: true });
			const rows = axisValuesForm(page).locator('tbody tr');
			await expect(rows).toHaveCount(0);
			await expect(axisValuesPreviewSummary(page, 0, 3)).toBeVisible();

			// listening は2件に出る。answer は1件にしか出ないので、既定では隠れる。
			const listening = wordCandidate(page, 'listening', 2);
			const answer = wordCandidate(page, 'answer', 1);
			await expect(listening).toBeVisible();
			await expect(answer).toHaveCount(0);

			await listening.click();
			await expect(rows).toHaveCount(1);
			await expect(rows.nth(0).getByRole('textbox').nth(0)).toHaveValue('listening');
			await expect(listening).toHaveCount(0);
			await expect(axisValuesPreviewSummary(page, 2, 1)).toBeVisible();

			// 未設定のファイルの例は、開くまで出ない。
			const example = axisValuesForm(page).getByText('2024/answer.pdf', { exact: true });
			await expect(example).toBeHidden();
			await axisValuesForm(page)
				.getByText(/^未設定のファイルの例$|^Examples of unset files$/)
				.click();
			await expect(example).toBeVisible();

			// 一覧に出ない語も、検索すると出る。
			const query = wordCandidatesRegion(page).getByRole('searchbox', {
				name: /^語を探す$|^Find a word$/
			});
			await query.fill('ANS');
			await expect(answer).toBeVisible();
			// 表にある語は、検索しても戻らない。
			await query.fill('listen');
			await expect(
				wordCandidatesRegion(page).getByRole('button', { name: /^listening/ })
			).toHaveCount(0);
			await expect(
				wordCandidatesRegion(page).getByText(/^当てはまる語はありません。$|^No matching words\.$/)
			).toBeVisible();
		}
	);
});

test('値の辞書: 照合する位置を行ごとに選ぶと一致する件数が変わり、保存すると残る', async ({
	page
}) => {
	await withDirectoryContent(
		page.request,
		{ type: 'archive', namePrefix: 'e2e-axis-match-position' },
		async (archive) => {
			await rescanArchive(page.request, archive.id);

			await page.goto(`/admin/contents/${archive.id}/axes`);
			await addAxis(page, '種類', { filenameWord: true });
			// ing は listening の末尾にだけ現れる。
			await addAxisValue(page, 'ing', '音声');
			await expect(axisValuesPreviewSummary(page, 2, 1)).toBeVisible();

			const position = axisValuesForm(page).getByRole('button', {
				name: /^ing の照合する位置: |^Match position for ing: /
			});
			const choose = async (option: RegExp) => {
				await position.click();
				await page.getByRole('option', { name: option }).click();
			};
			await choose(/^先頭か区切りの直後$|^Start or after a separator$/);
			await expect(axisValuesPreviewSummary(page, 0, 3)).toBeVisible();
			await choose(/^末尾$|^End$/);
			await expect(axisValuesPreviewSummary(page, 2, 1)).toBeVisible();

			await saveAxisValues(page, '種類');
			await page.reload();
			await selectAxis(page, '種類');
			await expect(position).toHaveText(/末尾|End/);
		}
	);
});

test('軸の削除: 確認して消せる。表示タイトルで使っている軸は消せずに理由が出る', async ({
	page
}) => {
	await withDirectoryContent(
		page.request,
		{ type: 'archive', namePrefix: 'e2e-axis-delete' },
		async (archive) => {
			// 年度・種類はテンプレートで使う。3本目はどこでも使わない。
			await setUpArchiveForBrowsing(page.request, archive);
			await send(page.request, 'post', `/contents/${archive.id}/axes`, {
				name: '未使用',
				source: 'dirLevel',
				dirLevel: 1,
				position: 2
			});

			await page.goto(`/admin/contents/${archive.id}/axes`);
			await axisRow(page, '種類')
				.getByRole('button', { name: /^種類 を削除\.\.\.$|^Delete 種類\.\.\.$/ })
				.click();
			await confirmDelete(page);
			const errorDialog = page.getByRole('alertdialog').filter({
				hasText:
					/軸「種類」は表示タイトルのテンプレートで使われているため削除できません。|The axis "種類" is used by the title template and cannot be deleted\./
			});
			await expect(errorDialog).toBeVisible();
			await errorDialog.getByRole('button', { name: CLOSE_BUTTON_NAME }).click();
			// エラーを閉じると削除の確認に戻る。やめて閉じる。
			await page
				.getByRole('alertdialog')
				.filter({ hasText: /この軸を削除しますか？|Delete this axis\?/ })
				.getByRole('button', { name: CANCEL_BUTTON_NAME })
				.click();
			await expect(page.getByRole('alertdialog')).toHaveCount(0);
			await expect(axisRow(page, '種類')).toBeVisible();

			await axisRow(page, '未使用')
				.getByRole('button', { name: /^未使用 を削除\.\.\.$|^Delete 未使用\.\.\.$/ })
				.click();
			await confirmDelete(page);
			await expect(page.getByRole('alertdialog')).toHaveCount(0);
			await expect(axisRow(page, '未使用')).toHaveCount(0);

			// 読み直しても消えたまま。残した軸は並ぶ (対照)。
			await page.reload();
			await expect(axisRow(page, '年度')).toBeVisible();
			await expect(axisRow(page, '種類')).toBeVisible();
			await expect(axisRow(page, '未使用')).toHaveCount(0);
		}
	);
});

test('一括公開: 絞り込んで全選択すると見えている行だけを公開し、全選択で非公開に戻せる', async ({
	page
}) => {
	await withDirectoryContent(
		page.request,
		{ type: 'archive', namePrefix: 'e2e-bulk-publish' },
		async (archive) => {
			await rescanArchive(page.request, archive.id);

			await page.goto(`/admin/contents/${archive.id}/items`);
			await expect(page.getByText(/3\s件\(公開 0\)|3 items \(0 published\)/)).toBeVisible();

			const filter = page.getByLabel(/^\s*(タイトルかパスで絞り込む|Filter by title or path)\s*$/);
			const selectAll = page.getByRole('checkbox', { name: /^すべて選択$|^Select all$/ });
			await filter.fill('2024/');
			await expect(page.getByText(/^2\s件を表示中$|^2 shown$/)).toBeVisible();
			await selectAll.click();
			await expect(page.getByText(/^2\s件選択中$|^2 selected$/)).toBeVisible();
			await page
				.getByRole('button', { name: /^選択した項目を公開する$|^Publish selected$/ })
				.click();
			await expect(page.getByText(/3\s件\(公開 2\)|3 items \(2 published\)/)).toBeVisible();

			// 読み直しても保たれ、絞り込みで隠れていた行は公開していない。
			await page.reload();
			await expect(page.getByText(/3\s件\(公開 2\)|3 items \(2 published\)/)).toBeVisible();
			await expect(itemPublishedSwitch(page, '2024/listening.mp3')).toHaveAttribute(
				'aria-checked',
				'true'
			);
			await expect(itemPublishedSwitch(page, '2023/listening.mp3')).toHaveAttribute(
				'aria-checked',
				'false'
			);

			await selectAll.click();
			await expect(page.getByText(/^3\s件選択中$|^3 selected$/)).toBeVisible();
			await page
				.getByRole('button', { name: /^選択した項目を非公開にする$|^Unpublish selected$/ })
				.click();
			await expect(page.getByText(/3\s件\(公開 0\)|3 items \(0 published\)/)).toBeVisible();

			// 非公開に戻したことも、読み直して確かめる。
			await page.reload();
			await expect(page.getByText(/3\s件\(公開 0\)|3 items \(0 published\)/)).toBeVisible();
			await expect(itemPublishedSwitch(page, '2024/listening.mp3')).toHaveAttribute(
				'aria-checked',
				'false'
			);
		}
	);
});

test('絞り込みの解除: 解除のリンクで絞り込みが外れ、全件に戻る', async ({ page }) => {
	await withDirectoryContent(
		page.request,
		{ type: 'archive', namePrefix: 'e2e-filter-reset' },
		async (archive) => {
			await setUpArchiveForBrowsing(page.request, archive);

			await page.goto(`/archives/${archive.id}`);
			await expect(page.getByText(/^3\s件$|^3 items$/)).toBeVisible();
			const resetLink = page.getByRole('link', { name: /^絞り込みを解除する$|^Clear filters$/ });
			// 絞り込んでいないときは出ない。
			await expect(resetLink).toHaveCount(0);

			const trigger = page.getByRole('button', { name: '年度', exact: true });
			await trigger.click();
			await page.getByRole('option', { name: /^2023/ }).click();
			await page.waitForURL((url) => url.searchParams.get('年度') === '2023');
			await expect(page.getByText(/^1\s件$|^1 items$/)).toBeVisible();

			await resetLink.click();
			await page.waitForURL((url) => !url.searchParams.has('年度'));
			await expect(page.getByText(/^3\s件$|^3 items$/)).toBeVisible();
			await expect(trigger).toContainText(/すべて|All/);
			await expect(resetLink).toHaveCount(0);
		}
	);
});
