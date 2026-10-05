/**
 * 開発用コンテンツデータ準備スクリプト（`node frontend/scripts/setup-content-data.ts`）。
 * 各種サンプルファイルをダウンロード・生成して `data/content/` 配下に配置する。
 *
 * 各ステップはファイル単位でスキップ判定する（再実行で失敗分のみ再取得できる）。
 * 例外: sample_files は zip 展開のためディレクトリ単位でスキップ（ディレクトリを削除して再実行）。
 * 特定のファイルを再取得するにはそのファイルを削除してから再実行すること。
 */

import { Buffer } from 'node:buffer';
import { execFileSync } from 'node:child_process';
import { platform } from 'node:os';
import { basename, join, relative, resolve } from 'node:path';
import { existsSync, mkdirSync, rmSync, unlinkSync, writeFileSync } from 'node:fs';
import process from 'node:process';
import { pathToFileURL } from 'node:url';
import * as cheerio from 'cheerio';
import { dataDir } from './data-dir.ts';

// zip展開にはGNU tarではなくzip形式を読めるbsdtar(libarchive)が要る。macOS/BSDは
// 標準tarがbsdtarだが、Windowsは`tar`という名前がPATH上でGit Bash付属のGNU tarに
// 解決されがちで、それはzipを読めない上にドライブレター`C:\...`を`host:path`の
// リモート指定と誤認識して失敗する。Windows 10(1803)以降はSystem32にbsdtar製の
// tar.exeが標準同梱されているため、PATH解決に頼らずWindowsではそちらを明示的に使う。
const TAR_BIN =
	platform() === 'win32'
		? join(process.env.SystemRoot ?? 'C:\\Windows', 'System32', 'tar.exe')
		: 'tar';

const YEAR_ROUND_RE = /(\d{4})年度\s*第(\d+)回/;
const KYOTSU_YEAR_PATH_RE = /\/(r\d+)\//;

// スクレイピング元ページの情報を素直にファイル名へ使うと、外部サイトが返す値次第で
// 保存先ディレクトリの外に書き込まれてしまう(パストラバーサル)。パス区切り文字や
// `.`/`..` を持ち込ませないよう、必ずこの関数を通してから join() に渡す。
function safeFilename(raw: string, fallback: string): string {
	const name = basename(raw.trim());
	if (!name || name === '.' || name === '..') return fallback;
	return name;
}

// safeFilename() を通していても、環境依存の記号(NULL文字等)混入まではケアできない。
// 保存直前に「解決後のパスが targetDir 配下に収まっているか」を最終防衛として確認する。
function isInsideDir(dir: string, filepath: string): boolean {
	const rel = relative(resolve(dir), resolve(filepath));
	return rel !== '' && !rel.startsWith('..') && !rel.startsWith(`..${'/'}`);
}

const FETCH_TIMEOUT_MS = 30_000;

// 外部サイトの応答が返らない・遅い場合にスクリプトが無期限に停止しないよう、
// すべての外部アクセスにタイムアウトを付ける共通ラッパー。
function fetchWithTimeout(url: string, init: RequestInit = {}): Promise<Response> {
	return fetch(url, { ...init, signal: AbortSignal.timeout(FETCH_TIMEOUT_MS) });
}

type DownloadLink = { href: string; text: string; type: 'pdf' | 'mp3' };

// スクレイピング対象ページが改変された場合に、ページ内リンクを辿って任意の
// 外部ホストからファイルを取得してしまわないよう、ダウンロード先ホストを
// 想定サイトに限定する。
function isAllowedHost(url: string, allowedHosts: readonly string[]): boolean {
	try {
		return allowedHosts.includes(new URL(url).hostname);
	} catch {
		return false;
	}
}

// isAllowedHost() でダウンロード直前の URL を検査していても、fetch は既定でリダイレクトを
// 追従するため、許可済みホストが 302 等で許可外ホストに転送すると素通りしてしまう。
// redirect: 'manual' で受け取り、Location を1段ごとに許可リストへ通してから追従する。
async function fetchAllowedHost(
	url: string,
	allowedHosts: readonly string[],
	init: RequestInit = {},
	maxRedirects = 5
): Promise<Response> {
	let currentUrl = url;
	for (let i = 0; i <= maxRedirects; i++) {
		if (!isAllowedHost(currentUrl, allowedHosts)) {
			throw new Error(`Disallowed host: ${currentUrl}`);
		}
		const response = await fetchWithTimeout(currentUrl, {
			...init,
			redirect: 'manual'
		});
		if (response.status >= 300 && response.status < 400) {
			const location = response.headers.get('location');
			if (!location) return response;
			currentUrl = new URL(location, currentUrl).href;
			continue;
		}
		return response;
	}
	throw new Error(`Too many redirects: ${url}`);
}

/** 許可済みサイトのリンクを、保存先を検査してから未取得分だけ保存する。 */
async function downloadAllowedLinks(
	pageUrl: string,
	targetDir: string,
	links: DownloadLink[],
	allowedHosts: readonly string[],
	rawFilename: (link: DownloadLink, absoluteUrl: string) => string,
	init: RequestInit = {}
): Promise<void> {
	for (const link of links) {
		const absoluteUrl = new URL(link.href, pageUrl).href;
		if (!isAllowedHost(absoluteUrl, allowedHosts)) {
			console.log(`      ⚠️  Skip: disallowed host in ${absoluteUrl}`);
			continue;
		}

		const filename = safeFilename(rawFilename(link, absoluteUrl), `unknown.${link.type}`);
		const filepath = join(targetDir, filename);
		if (!isInsideDir(targetDir, filepath)) {
			console.log(`      ⚠️  Skip: unsafe filename derived from ${absoluteUrl}`);
			continue;
		}
		if (existsSync(filepath)) {
			console.log(`      ⏭️  Skip: ${filename} (already exists)`);
			continue;
		}

		try {
			const response = await fetchAllowedHost(absoluteUrl, allowedHosts, init);
			if (response.ok) {
				writeFileSync(filepath, Buffer.from(await response.arrayBuffer()));
				console.log(`      ✅ Downloaded: ${filename}`);
			} else {
				console.log(`      ⚠️  Failed: ${filename} (HTTP ${response.status})`);
			}
		} catch (error) {
			console.log(`      ❌ Error: ${filename}`, error);
		}
	}
}

/**
 * テストデータ生成: 汎用テストファイルをダウンロード
 * 画像・テキスト・マークダウンなど、sample_files では不足するファイル種別を補う
 */
async function setupGeneralFiles() {
	const generalDir = join(contentDir, 'general');
	mkdirSync(generalDir, { recursive: true });

	const downloads = [
		{
			filename: 'svelte-readme.md',
			url: 'https://raw.githubusercontent.com/sveltejs/svelte/refs/heads/main/README.md'
		},
		// Lorem Picsum のテスト用画像（seed 固定で再現性確保・CC ライセンス・直リンク永続）。
		// 異なるアスペクト比を揃え、og:image / サムネイル生成 / 一覧表示などの e2e で使う
		{
			filename: 'photo.jpg',
			url: 'https://picsum.photos/seed/mosk-test/1920/1080.jpg'
		},
		{
			filename: 'portrait.jpg',
			url: 'https://picsum.photos/seed/mosk-portrait/1080/1920.jpg'
		},
		{
			filename: 'thumbnail.jpg',
			url: 'https://picsum.photos/seed/mosk-thumb/400/400.jpg'
		},
		{
			filename: 'wallpaper.png',
			url: 'https://placehold.co/1920x1080.png'
		},
		// SoundHelix のテスト用楽曲（CC ライセンス・直リンク永続）。audio player の e2e 等で利用
		{
			filename: 'SoundHelix-Song-1.mp3',
			url: 'https://www.soundhelix.com/examples/mp3/SoundHelix-Song-1.mp3'
		},
		{
			filename: 'SoundHelix-Song-2.mp3',
			url: 'https://www.soundhelix.com/examples/mp3/SoundHelix-Song-2.mp3'
		}
	];

	const svgPath = join(generalDir, 'icon.svg');
	if (!existsSync(svgPath)) {
		writeFileSync(
			svgPath,
			'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 200 200" width="200" height="200">' +
				'<rect width="200" height="200" fill="#e8f0fe"/>' +
				'<rect x="20" y="40" width="160" height="120" rx="8" fill="#4285f4"/>' +
				'<circle cx="72" cy="90" r="22" fill="#fff"/>' +
				'<polygon points="28,158 92,98 132,138 158,108 188,158" fill="#34a853"/>' +
				'<text x="100" y="192" text-anchor="middle" font-size="14" fill="#4285f4" font-family="sans-serif">icon.svg</text>' +
				'</svg>'
		);
		console.log('  ✅ Generated: icon.svg');
	} else {
		console.log('  ⏭️  Skip: icon.svg (already exists)');
	}

	console.log('📦 Downloading general test files...');

	await downloadFiles(generalDir, downloads);

	console.log('✅ General test files created');
}

/** `downloads` を `dir` に取ってくる。既にあるファイルは取り直さない。 */
async function downloadFiles(dir: string, downloads: { filename: string; url: string }[]) {
	for (const { filename, url } of downloads) {
		const filepath = join(dir, filename);
		if (existsSync(filepath)) {
			console.log(`  ⏭️  Skip: ${filename} (already exists)`);
			continue;
		}
		try {
			const response = await fetchWithTimeout(url);
			if (response.ok) {
				writeFileSync(filepath, Buffer.from(await response.arrayBuffer()));
				console.log(`  ✅ Downloaded: ${filename}`);
			} else {
				console.log(`  ⚠️  Failed: ${filename} (HTTP ${response.status})`);
			}
		} catch (error) {
			console.log(`  ❌ Error: ${filename}`, error);
		}
	}
}

/**
 * テストデータ生成: 動画・PDF 等のメディアファイルをダウンロード
 * 動画ソース:
 *   - Big Buck Bunny (Blender Foundation, CC BY 3.0) の W3Schools ホスト版
 *   - filesamples.com: 無料・登録不要のサンプルファイル配布サービス
 */
async function setupMediaFiles() {
	const mediaDir = join(contentDir, 'general');
	mkdirSync(mediaDir, { recursive: true });

	const downloads = [
		// Big Buck Bunny クリップ（Blender Foundation, CC BY 3.0）— W3Schools ホスト
		{
			filename: 'sample.mp4',
			url: 'https://www.w3schools.com/html/mov_bbb.mp4'
		},
		// filesamples.com — 無料・登録不要のサンプルファイル配布サービス
		{
			filename: 'sample.webm',
			url: 'https://filesamples.com/samples/video/webm/sample_640x360.webm'
		},
		{
			filename: 'sample.ogv',
			url: 'https://filesamples.com/samples/video/ogv/sample_640x360.ogv'
		},
		{
			filename: 'sample.pdf',
			url: 'https://filesamples.com/samples/document/pdf/sample1.pdf'
		}
	];

	console.log('📦 Downloading media test files...');

	await downloadFiles(mediaDir, downloads);

	console.log('✅ Media test files created');
}

/**
 * テストデータ生成: sample_files をダウンロード・展開
 * https://sample.add.sh/ からzipファイルをダウンロードして展開
 */
async function setupSampleFiles() {
	const sampleFilesDir = join(contentDir, 'sample_files');

	// 既に存在する場合はスキップ
	if (existsSync(sampleFilesDir)) {
		console.log('⏭️  Skip: sample_files already exists');
		return;
	}

	console.log('📦 Downloading sample_files.zip...');

	const zipPath = join(contentDir, 'sample_files.zip');
	try {
		const zipUrl = 'https://sample.add.sh/sample_files.zip';
		const response = await fetchWithTimeout(zipUrl);

		if (!response.ok) {
			throw new Error(`Failed to download: HTTP ${response.status}`);
		}

		const arrayBuffer = await response.arrayBuffer();
		const buffer = Buffer.from(arrayBuffer);

		writeFileSync(zipPath, buffer);
		console.log('✅ Downloaded sample_files.zip');

		// 展開前にエントリ名を検査し、絶対パスや `..` を含む(zip-slip)ものが
		// あれば展開せず中断する。execFileSync はシェルを経由しないため引数の
		// インジェクションも発生しない。
		const listing = execFileSync(TAR_BIN, ['-tf', zipPath], { encoding: 'utf-8' });
		const unsafeEntry = listing
			.split('\n')
			.filter(Boolean)
			.find((entry) => entry.startsWith('/') || entry.split('/').includes('..'));
		if (unsafeEntry) {
			throw new Error(`Unsafe entry in sample_files.zip: ${unsafeEntry}`);
		}

		// エントリ名だけの検査では、シンボリックリンクの「リンク先」が展開先の
		// 外を指すケースを検出できない(リンク先パスは `..` チェックの対象外)。
		// `-tvf` の種別表示(先頭1文字)で通常ファイル/ディレクトリ以外を拒否する。
		const verboseListing = execFileSync(TAR_BIN, ['-tvf', zipPath], { encoding: 'utf-8' });
		const unsafeTypeEntry = verboseListing
			.split('\n')
			.filter(Boolean)
			.find((line) => line[0] !== '-' && line[0] !== 'd');
		if (unsafeTypeEntry) {
			throw new Error(`Unsafe entry type (symlink等) in sample_files.zip: ${unsafeTypeEntry}`);
		}

		console.log('📂 Extracting sample_files.zip...');
		mkdirSync(sampleFilesDir, { recursive: true });
		// tar(bsdtar) は zip 展開もクロスプラットフォームに扱えるため unzip コマンドに依存しない
		execFileSync(TAR_BIN, ['-xf', zipPath, '-C', sampleFilesDir], { stdio: 'inherit' });
		console.log('✅ Sample files data created');
	} catch (error) {
		console.error('❌ Error setting up sample files:', error);
		// 展開が途中で失敗すると、次回実行時に「ディレクトリが存在する」だけで
		// 丸ごとスキップされてしまう。中途半端な状態を残さないよう削除する。
		if (existsSync(sampleFilesDir)) {
			rmSync(sampleFilesDir, { recursive: true, force: true });
		}
		throw error;
	} finally {
		if (existsSync(zipPath)) {
			unlinkSync(zipPath);
		}
	}
}

// PDF は www.eiken.or.jp、リスニング音声(MP3)は別サブドメインの media.eiken.or.jp から配信される。
const ALLOWED_EIKEN_HOSTS = ['www.eiken.or.jp', 'media.eiken.or.jp'];

/**
 * テストデータ生成: 英検の過去問（PDF・MP3）をダウンロード
 */
async function setupEikenData() {
	const eikenDir = join(contentDir, 'eiken');
	mkdirSync(eikenDir, { recursive: true });

	const grades = [
		{ name: '1級', path: 'grade_1' },
		{ name: '準1級', path: 'grade_p1' },
		{ name: '2級', path: 'grade_2' },
		{ name: '準2級プラス', path: 'grade_p2plus' },
		{ name: '準2級', path: 'grade_p2' },
		{ name: '3級', path: 'grade_3' },
		{ name: '4級', path: 'grade_4' },
		{ name: '5級', path: 'grade_5' }
	];

	console.log('📚 Downloading Eiken past exam data...');

	try {
		for (const grade of grades) {
			console.log(`  📖 Accessing ${grade.name} page...`);
			const url = `https://www.eiken.or.jp/eiken/exam/${grade.path}/`;

			try {
				const response = await fetchWithTimeout(url);
				if (!response.ok) {
					console.log(`  ⚠️  Failed to access ${grade.name} page (HTTP ${response.status})`);
					continue;
				}
				const html = await response.text();
				const $ = cheerio.load(html);

				const examSections: Array<{
					year: string;
					round: string;
					links: DownloadLink[];
				}> = [];

				// h4タグ（例: "2025年度 第2回"）を全て取得し、年度・回次を抽出
				$('h4').each((_, h4Element) => {
					const headingText = $(h4Element).text().trim();
					const match = headingText.match(YEAR_ROUND_RE);
					if (!match) return;

					const year = match[1];
					const round = match[2];

					const links: DownloadLink[] = [];

					const h4Container = $(h4Element).parent();
					if (h4Container.length === 0) return;

					// div.c-h4 の兄弟要素を、次の div.c-h4 またはアコーディオンまで探索
					let sibling = h4Container.next();
					while (sibling.length > 0) {
						if (sibling.hasClass('c-h4') || sibling.hasClass('c-accordion-box')) break;

						sibling.find('a[href$=".pdf"]').each((_, link) => {
							links.push({
								href: $(link).attr('href') || '',
								text: $(link).text().trim(),
								type: 'pdf'
							});
						});

						sibling.find('a[href$=".mp3"]').each((_, link) => {
							links.push({
								href: $(link).attr('href') || '',
								text: $(link).text().trim(),
								type: 'mp3'
							});
						});

						sibling = sibling.next();
					}

					if (links.length > 0) {
						examSections.push({ year, round, links });
					}
				});

				$('.c-accordion-box').each((_, accordion) => {
					const headingText = $(accordion).find('.c-accordion-head p').text().trim();
					const match = headingText.match(YEAR_ROUND_RE);
					if (!match) return;

					const year = match[1];
					const round = match[2];

					const links: DownloadLink[] = [];

					$(accordion)
						.find('a[href$=".pdf"]')
						.each((_, link) => {
							links.push({
								href: $(link).attr('href') || '',
								text: $(link).text().trim(),
								type: 'pdf'
							});
						});

					$(accordion)
						.find('a[href$=".mp3"]')
						.each((_, link) => {
							links.push({
								href: $(link).attr('href') || '',
								text: $(link).text().trim(),
								type: 'mp3'
							});
						});

					if (links.length > 0) {
						examSections.push({ year, round, links });
					}
				});

				for (const section of examSections) {
					const { year, round, links } = section;
					const targetDir = join(eikenDir, year, round, grade.path);
					mkdirSync(targetDir, { recursive: true });

					console.log(`    📅 Year ${year} Round ${round} (${links.length} files)`);

					await downloadAllowedLinks(
						url,
						targetDir,
						links,
						ALLOWED_EIKEN_HOSTS,
						(_link, absoluteUrl) => absoluteUrl.split('/').pop() ?? ''
					);
				}

				console.log(`  ✅ ${grade.name} completed (${examSections.length} sections)`);
			} catch (error) {
				console.log(`  ❌ Error processing ${grade.name}:`, error);
			}
		}
	} catch (error) {
		console.error('❌ Error fetching Eiken data:', error);
	}

	console.log('✅ Eiken test data created');
}

const ALLOWED_KYOTSU_HOSTS = ['www.dnc.ac.jp'];

/** 共通テストの URL から、サイトが指定したダウンロード名を取り出す。 */
function kyotsuFilename(link: DownloadLink, pageUrl: string): string {
	try {
		const url = new URL(link.href, pageUrl);
		const nParam = url.searchParams.get('n');
		if (nParam) return decodeURIComponent(nParam);
		return link.href.split('/').pop()?.split('?')[0] || `unknown.${link.type}`;
	} catch {
		return link.href.split('=').pop()?.split('&')[0] || `unknown.${link.type}`;
	}
}

/**
 * テストデータ生成: 大学入学共通テストの過去問（PDF）をダウンロード
 */
async function setupKyotsuData() {
	const kyotsuDir = join(contentDir, 'kyotsu');
	mkdirSync(kyotsuDir, { recursive: true });

	const headers = {
		'User-Agent':
			'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36'
	};

	console.log('📚 Downloading Kyotsu test past exam data...');

	try {
		const topUrl = 'https://www.dnc.ac.jp/kyotsu/kakomondai/';
		const topResponse = await fetchWithTimeout(topUrl, { headers });
		if (!topResponse.ok) {
			console.log(`  ⚠️  Failed to access top page (HTTP ${topResponse.status})`);
			console.log('✅ Kyotsu test data created');
			return;
		}
		const topHtml = await topResponse.text();
		const $ = cheerio.load(topHtml);

		// 年度リンク（r7, r6, r5 など）を抽出
		const years: string[] = [];
		$('a[href*="/kyotsu/kakomondai/r"]').each((_, link) => {
			const href = $(link).attr('href') || '';
			const match = href.match(KYOTSU_YEAR_PATH_RE);
			if (match && !years.includes(match[1])) {
				years.push(match[1]);
			}
		});

		console.log(`  Found ${years.length} years: ${years.join(', ')}`);

		for (const year of years) {
			console.log(`  📖 Processing ${year}...`);
			const yearDir = join(kyotsuDir, year);
			mkdirSync(yearDir, { recursive: true });

			const examTypes = [
				{ type: 'honshiken', label: 'Main exam', paths: ['mondai', 'seikai'] },
				{ type: 'tuisaishiken', label: 'Supplementary exam', paths: ['mondai', 'seikai'] }
			];

			for (const examType of examTypes) {
				for (const path of examType.paths) {
					const pageUrl = `https://www.dnc.ac.jp/kyotsu/kakomondai/${year}/${year}_${examType.type}_${path}.html`;
					const targetDir = join(yearDir, examType.type, path);

					try {
						const pageResponse = await fetchWithTimeout(pageUrl, { headers });
						if (!pageResponse.ok) {
							console.log(`    ⏭️  Skip: ${examType.label} ${path} (not found)`);
							continue;
						}

						const pageHtml = await pageResponse.text();
						const page$ = cheerio.load(pageHtml);

						const fileLinks: DownloadLink[] = [];
						page$('a[href$=".pdf"]').each((_, link) => {
							fileLinks.push({
								href: page$(link).attr('href') || '',
								text: page$(link).text().trim(),
								type: 'pdf'
							});
						});
						page$('a[href$=".mp3"]').each((_, link) => {
							fileLinks.push({
								href: page$(link).attr('href') || '',
								text: page$(link).text().trim(),
								type: 'mp3'
							});
						});

						if (fileLinks.length === 0) {
							console.log(`    ⏭️  Skip: ${examType.label} ${path} (no files)`);
							continue;
						}

						mkdirSync(targetDir, { recursive: true });
						console.log(`    📅 ${examType.label} ${path} (${fileLinks.length} files)`);

						await downloadAllowedLinks(
							pageUrl,
							targetDir,
							fileLinks,
							ALLOWED_KYOTSU_HOSTS,
							(link) => kyotsuFilename(link, pageUrl),
							{ headers }
						);
					} catch (error) {
						console.log(`    ❌ Error accessing ${examType.label} ${path}:`, error);
					}
				}
			}

			console.log(`  ✅ ${year} completed`);
		}
	} catch (error) {
		console.error('❌ Error fetching Kyotsu data:', error);
	}

	console.log('✅ Kyotsu test data created');
}

/**
 * テストデータ生成: 千葉県公立高校入試過去問データをダウンロード
 * URL の命名規則が年度によって揺れる（national/kokugo, answer/answersheet, 年サフィックスの有無）ため、
 * 候補 URL を順に試して最初に成功したものを採用するトライ＆エラー方式。
 *
 * ただし年サフィックス無しの URL は「その時点で公開されている最新の年度」を指しているだけで、
 * 過去年度の内容を保証しない。全年度で無条件にフォールバックを許すと、年サフィックス無し URL
 * しか無い科目が全年度で同一ファイル(=最新のもの)になってしまうため、フォールバックは
 * 収集対象の最新年度(CHIBA_YEAR_END)にのみ許可する。
 */
const CHIBA_YEAR_START = 2021;
const CHIBA_YEAR_END = 2026;

async function setupChibaData() {
	const chibaDir = join(contentDir, 'chiba');
	mkdirSync(chibaDir, { recursive: true });

	const BASE = 'https://www.chibanippo.co.jp/pdf';
	const subjects = [
		{ dir: 'kokugo', names: ['kokugo', 'national'] },
		{ dir: 'sugaku', names: ['sugaku'] },
		{ dir: 'eigo', names: ['english'] },
		{ dir: 'rika', names: ['science'] },
		{ dir: 'shakai', names: ['society'] }
	];
	const types = [
		{ dir: 'question', suffixes: ['question'] },
		{ dir: 'answer', suffixes: ['answer', 'answersheet'] }
	];

	console.log('📚 Downloading Chiba Prefecture public high school entrance exam data...');

	try {
		for (let year = CHIBA_YEAR_START; year <= CHIBA_YEAR_END; year++) {
			console.log(`  📖 Processing ${year}...`);
			const yearDir = join(chibaDir, String(year));
			mkdirSync(yearDir, { recursive: true });

			let downloadedCount = 0;
			let skippedCount = 0;
			const isLatestYear = year === CHIBA_YEAR_END;

			for (const subject of subjects) {
				for (const type of types) {
					const filename = `${subject.dir}_${type.dir}.pdf`;
					const filepath = join(yearDir, filename);

					if (existsSync(filepath)) {
						console.log(`    ⏭️  Skip: ${filename} (already exists)`);
						skippedCount++;
						continue;
					}

					const candidateUrls = subject.names.flatMap((name) =>
						type.suffixes.flatMap((suffix) => {
							const urls = [`${BASE}/${name}_${suffix}${year}.pdf`];
							// 年サフィックス無し URL は最新年度の内容を指すため、最新年度の
							// フォールバックとしてのみ候補に加える(過去年度には使わない)。
							if (isLatestYear) {
								urls.push(`${BASE}/${name}_${suffix}.pdf`);
							}
							return urls;
						})
					);

					let downloaded = false;
					for (const url of candidateUrls) {
						try {
							const response = await fetchWithTimeout(url);
							if (response.ok) {
								const arrayBuffer = await response.arrayBuffer();
								writeFileSync(filepath, Buffer.from(arrayBuffer));
								console.log(`    ✅ Downloaded: ${filename}`);
								downloadedCount++;
								downloaded = true;
								break;
							}
						} catch {
							// ignore and try next candidate
						}
					}

					if (!downloaded) {
						console.log(`    ⚠️  Not found: ${filename}`);
					}
				}
			}

			console.log(
				`  ✅ ${year} completed (${downloadedCount} downloaded, ${skippedCount} skipped)`
			);
		}
	} catch (error) {
		console.error('❌ Error fetching Chiba data:', error);
	}

	console.log('✅ Chiba Prefecture test data created');
}

/**
 * メイン処理: すべてのコンテンツデータを生成
 */
async function main() {
	console.log('🚀 Setting up content data...\n');

	try {
		await setupGeneralFiles();
		await setupMediaFiles();
		await setupSampleFiles();
		await setupEikenData();
		await setupKyotsuData();
		await setupChibaData();

		console.log('\n✨ All content data has been set up successfully!');
	} catch (error) {
		console.error('❌ Error setting up content data:', error);
		process.exit(1);
	}
}

const contentDir = join(dataDir(), 'content');

// スクリプトとして直接実行されたかチェック（URL同士で比較、クロスプラットフォーム対応）
const isMainModule = process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href;

if (isMainModule) {
	await main();
}
