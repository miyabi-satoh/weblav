// `just flip-videos` から呼ばれる。画面の操作を1つずつ撮り、1秒に1コマで切り替わる動画にまとめる。
// 実機で画面を追う確認の一部を、動画を見ることで代えるためのもの。
//
// 1. 使い捨ての backend を立て、仕様書と同じシードとフィクスチャを入れる (→ shot-fixtures.ts)。
// 2. 筋書き (→ flip-stories.ts) を、役割 (ログインしない閲覧者・編集者・管理者) ごとに通し、操作のたびに撮る。
//    コマの上の帯に、筋書き・役割・操作の説明を入れる。
// 3. 筋書きと画面の幅ごとに、ffmpeg で mp4 にする。役割は1本の中で順に流す。
//    同じ名前の .sheet.png に全コマを並べる。撮り直したら、報告の前に全本の一覧を見て、
//    意図しない画面 (エラーやトースト) が写っていないかを確かめるため。
//
// 名前を渡すと、その筋書きだけを撮る (`just flip-videos browse admin`)。
// 出力先は docs/generated/flip/ (.gitignore 対象)。環境変数 WEBLAV_FLIP_DIR で変えられる。

import { spawnSync } from 'node:child_process';
import { mkdirSync, mkdtempSync, rmSync } from 'node:fs';
import path from 'node:path';
import { chromium, type Browser, type Page } from 'playwright';
import { startBackend, type Backend } from './backend-process.ts';
import { stories, type Role, type Story } from './flip-stories.ts';
import { setUpFixtures, writeFixtureFiles } from './shot-fixtures.ts';
import { VIEWPORTS, type ViewportName } from './shots.ts';
import { TEST_ADMIN, TEST_USER } from './test-account.ts';

const REPO_ROOT = path.resolve(import.meta.dirname, '../..');
const OUT_DIR = path.resolve(REPO_ROOT, process.env.WEBLAV_FLIP_DIR ?? 'docs/generated/flip');
/** コマの置き場。片付け漏れが普段見ない場所に溜まらないよう、target/ の下にする。 */
const FRAMES_ROOT = path.join(REPO_ROOT, 'target/flip-frames');

/** 帯の高さ (CSS px)。説明が2行まで入る。 */
const BAND_HEIGHT = 72;
/** 一覧の画像の1コマの幅の上限 (px)。帯の文字が読める大きさに留める。 */
const SHEET_FRAME_WIDTH = 640;
/** 1コマを見せる秒数。 */
const SECONDS_PER_FRAME = 1;
/** 操作の中の待ちの上限。画面を変えて要素が見つからなくなったとき、長く待たずに止める。 */
const STEP_TIMEOUT_MS = 10_000;
/** 操作のあと、色の切り替えやトーストが出そろうのを待つ時間。 */
const SETTLE_MS = 300;

const VIEWPORT_LABELS: Record<ViewportName, string> = {
	desktop: 'PC',
	tablet: 'タブレット',
	mobile: 'スマートフォン'
};
/** スマートフォンは幅が狭く、等倍では文字が潰れるので2倍で撮る。 */
const SCALE: Record<ViewportName, number> = { desktop: 1, tablet: 1, mobile: 2 };

const ROLES: Record<
	Role,
	{ label: string; color: string; account: { username: string; password: string } | null }
> = {
	anonymous: { label: '閲覧者 (ログインなし)', color: '#6b7280', account: null },
	user: { label: '編集者', color: '#2563eb', account: TEST_USER },
	admin: { label: '管理者', color: '#b45309', account: TEST_ADMIN }
};

const ONLY = process.argv.slice(2);
const unknown = ONLY.filter((name) => !Object.hasOwn(stories, name));
if (unknown.length > 0) {
	// 名前を打ち間違えたまま、何も撮らずに成功で終えないため。
	console.error(
		`無い筋書きの名前です: ${unknown.join(', ')}\n撮れるもの: ${Object.keys(stories).join(', ')}`
	);
	process.exit(1);
}
if (spawnSync('ffmpeg', ['-version'], { stdio: 'ignore' }).status !== 0) {
	console.error('ffmpeg が見つかりません。入れてから流し直してください。');
	process.exit(1);
}

const escapeHtml = (text: string) =>
	text.replace(/[&<>"]/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' })[c]!);

/** 撮った画面の上に帯を付けて、コマとして書き出す。 */
async function writeFrame(
	composer: Page,
	file: string,
	screenshot: Buffer,
	band: { story: string; viewport: ViewportName; role: Role; position: string; caption: string }
) {
	const role = ROLES[band.role];
	await composer.setContent(`<!doctype html>
<style>
	* { box-sizing: border-box; margin: 0; }
	body { font-family: system-ui, sans-serif; background: #111827; }
	.band { height: ${BAND_HEIGHT}px; padding: 8px 12px; color: #f9fafb; overflow: hidden; }
	.meta { display: flex; gap: 8px; align-items: center; font-size: 12px; line-height: 20px; white-space: nowrap; }
	.role { padding: 0 8px; border-radius: 10px; background: ${role.color}; font-weight: 600; }
	.story { overflow: hidden; text-overflow: ellipsis; color: #d1d5db; }
	.position { margin-left: auto; color: #d1d5db; font-variant-numeric: tabular-nums; }
	.caption { margin-top: 2px; font-size: 14px; line-height: 17px; font-weight: 600; }
	img { display: block; width: 100%; }
</style>
<div class="band">
	<div class="meta">
		<span class="role">${escapeHtml(role.label)}</span>
		<span class="story">${escapeHtml(band.story)} ・ ${VIEWPORT_LABELS[band.viewport]}</span>
		<span class="position">${band.position}</span>
	</div>
	<div class="caption">${escapeHtml(band.caption)}</div>
</div>
<img src="data:image/png;base64,${screenshot.toString('base64')}" />`);
	await composer.locator('img').evaluate((img: HTMLImageElement) => img.decode());
	await composer.screenshot({ path: file });
}

/** 1つの筋書きを、1つの画面の幅で、役割ごとに通してコマを書き出す。コマの数を返す。 */
async function shootStory(
	browser: Browser,
	backend: Backend,
	name: string,
	story: Story,
	viewport: ViewportName,
	framesDir: string
): Promise<number> {
	const fixtures = await setUpFixtures(backend);
	const size = VIEWPORTS[viewport];
	const deviceScaleFactor = SCALE[viewport];
	const composerContext = await browser.newContext({
		viewport: { width: size.width, height: size.height + BAND_HEIGHT },
		deviceScaleFactor
	});
	const composer = await composerContext.newPage();
	let frame = 0;

	for (const role of story.roles) {
		const { account } = ROLES[role];
		// 役割ごとに context を分ける。ログインも、端末に覚えさせた表示の切り替えも持ち越さない。
		// locale と表示言語の cookie を固定するのは、実行する PC の言語で画面が変わらないようにするため。
		const context = await browser.newContext({
			viewport: size,
			deviceScaleFactor,
			baseURL: backend.baseURL,
			locale: 'ja-JP'
		});
		await context.addCookies([{ name: 'WEBLAV_LOCALE', value: 'ja', url: backend.baseURL }]);
		// 使い捨ての backend は 127.0.0.1 で待ち受けるので、そのままだと共有の画面が QR コードでなく
		// 「他の端末からはつながりません」の案内になる。他の端末から開ける設置と同じ答えを返させる。
		await context.route('**/api/v1/connection-info', (route) =>
			route.fulfill({ json: { mdnsHostname: 'home-pc.local', port: 3000 } })
		);
		if (account && !story.startsLoggedOut) {
			const res = await context.request.post('/api/v1/auth/login', { data: account });
			if (!res.ok()) throw new Error(`${role} でログインできませんでした (${res.status()})`);
		}
		const page = await context.newPage();
		page.setDefaultTimeout(STEP_TIMEOUT_MS);
		const steps = story.steps.filter((step) => !step.roles || step.roles.includes(role));
		for (const [i, step] of steps.entries()) {
			try {
				await step.run(page, { fixtures, role, viewport, account });
				await page.evaluate(() => document.fonts.ready);
				await page.waitForTimeout(SETTLE_MS);
				// ボタンにホバーの色が付かないよう、ポインターを外してから撮る。
				await page.mouse.move(1, 1);
				const screenshot = await page.screenshot({ animations: 'disabled' });
				frame += 1;
				await writeFrame(
					composer,
					path.join(framesDir, `${String(frame).padStart(4, '0')}.png`),
					screenshot,
					{
						story: story.title,
						viewport,
						role,
						position: `${i + 1} / ${steps.length}`,
						caption: step.caption
					}
				);
			} catch (err) {
				throw new Error(`${name} (${viewport}・${role}) の「${step.caption}」で止まりました`, {
					cause: err
				});
			}
		}
		await context.close();
	}
	await composerContext.close();
	return frame;
}

function encode(framesDir: string, outFile: string) {
	const result = spawnSync(
		'ffmpeg',
		[
			'-y',
			'-loglevel',
			'error',
			'-framerate',
			String(1 / SECONDS_PER_FRAME),
			'-i',
			path.join(framesDir, '%04d.png'),
			// 再生する側が低いフレームレートを扱えないことがあるので、30fps に複製して書き出す。
			// yuv420p と偶数の縦横は、ブラウザーや OS の標準のプレイヤーで再生できる形にするため。
			'-vf',
			'fps=30,scale=trunc(iw/2)*2:trunc(ih/2)*2,format=yuv420p',
			'-c:v',
			'libx264',
			'-movflags',
			'+faststart',
			outFile
		],
		{ stdio: 'inherit' }
	);
	if (result.status !== 0) throw new Error(`ffmpeg が失敗しました: ${outFile}`);
}

/**
 * 全コマを縮めて格子に並べた1枚の画像にする。
 * 列の数は、一覧がほぼ正方形になるようにコマの縦横比から決める。縦に長いと、画像を見る側で縮められて帯が読めなくなるため。
 */
function writeSheet(framesDir: string, frames: number, viewport: ViewportName, outFile: string) {
	const { width, height } = VIEWPORTS[viewport];
	const columns = Math.min(frames, Math.ceil(Math.sqrt((frames * (height + BAND_HEIGHT)) / width)));
	const rows = Math.ceil(frames / columns);
	const result = spawnSync(
		'ffmpeg',
		[
			'-y',
			'-loglevel',
			'error',
			'-i',
			path.join(framesDir, '%04d.png'),
			'-vf',
			`scale='min(${SHEET_FRAME_WIDTH},iw)':-2,tile=${columns}x${rows}:padding=8:color=white`,
			'-frames:v',
			'1',
			outFile
		],
		{ stdio: 'inherit' }
	);
	if (result.status !== 0) throw new Error(`ffmpeg が失敗しました: ${outFile}`);
}

async function main() {
	writeFixtureFiles();
	rmSync(FRAMES_ROOT, { recursive: true, force: true });
	mkdirSync(FRAMES_ROOT, { recursive: true });
	mkdirSync(OUT_DIR, { recursive: true });

	// Ctrl-C はこちらで受けて片付ける。Playwright に先にブラウザーを閉じさせない。
	const browser = await chromium.launch({ handleSIGINT: false });
	let backend: Backend | undefined;
	const cleanup = async () => {
		await browser.close();
		await backend?.stop();
		rmSync(FRAMES_ROOT, { recursive: true, force: true });
	};
	process.on('SIGINT', () => {
		void cleanup().finally(() => process.exit(130));
	});

	try {
		const names = ONLY.length > 0 ? ONLY : Object.keys(stories);
		for (const name of names) {
			for (const viewport of Object.keys(VIEWPORTS) as ViewportName[]) {
				// 筋書きはコンテンツやユーザーを足したり消したりするので、動画ごとに新しい backend で撮る。
				backend = await startBackend('weblav-flip-');
				const framesDir = mkdtempSync(path.join(FRAMES_ROOT, `${name}-${viewport}-`));
				const frames = await shootStory(browser, backend, name, stories[name], viewport, framesDir);
				await backend.stop();
				backend = undefined;
				const outFile = path.join(OUT_DIR, `${name}_${viewport}.mp4`);
				encode(framesDir, outFile);
				const sheetFile = path.join(OUT_DIR, `${name}_${viewport}.sheet.png`);
				writeSheet(framesDir, frames, viewport, sheetFile);
				console.log(
					`[flip] ${path.relative(REPO_ROOT, outFile)} (${frames} コマ。一覧: ${path.basename(sheetFile)})`
				);
			}
		}
	} finally {
		await cleanup();
	}
}

main().catch((err) => {
	console.error(err);
	process.exit(1);
});
