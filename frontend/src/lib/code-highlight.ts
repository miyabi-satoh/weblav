/**
 * テキストのビューアーで、ソースに言語に合わせた色を付ける (→ docs/ui.md「PDF・動画・テキストのビューアー」)。
 * highlight.js の本体と言語の定義は、色を付けるファイルを開いたときに読み込む。
 */

import type { HLJSApi, LanguageFn } from 'highlight.js';
import { fileExtension } from '$lib/file-kind';

type LanguageModule = { default: LanguageFn };

/** Markdown のファイルの拡張子。プレビューで組むか (→ $lib/markdown-preview.ts) もこれで決める。 */
export const MARKDOWN_EXTENSIONS = ['md', 'markdown'];

/** 拡張子 → highlight.js の言語名。言語の定義は使う分だけ読み込むので、ここで読み込み方も持つ。 */
const LANGUAGES: Record<string, [string, () => Promise<LanguageModule>]> = {};

/** highlight.js の言語名 → 読み込み方。Markdown のコードの塊 (```` ```yaml ````) は、拡張子か言語名で書かれる。 */
const NAMES: Record<string, [string, () => Promise<LanguageModule>]> = {};

function add(name: string, load: () => Promise<LanguageModule>, extensions: string[]) {
	NAMES[name] = [name, load];
	for (const extension of extensions) LANGUAGES[extension] = [name, load];
}

add('bash', () => import('highlight.js/lib/languages/bash'), ['sh', 'bash', 'zsh', 'shell']);
add('c', () => import('highlight.js/lib/languages/c'), ['c', 'h']);
add('cpp', () => import('highlight.js/lib/languages/cpp'), ['cpp', 'cc', 'cxx', 'hpp', 'hh']);
add('csharp', () => import('highlight.js/lib/languages/csharp'), ['cs']);
add('css', () => import('highlight.js/lib/languages/css'), ['css']);
add('diff', () => import('highlight.js/lib/languages/diff'), ['diff', 'patch']);
add('dos', () => import('highlight.js/lib/languages/dos'), ['bat', 'cmd']);
add('go', () => import('highlight.js/lib/languages/go'), ['go']);
add('ini', () => import('highlight.js/lib/languages/ini'), ['ini', 'toml']);
add('java', () => import('highlight.js/lib/languages/java'), ['java']);
add('javascript', () => import('highlight.js/lib/languages/javascript'), [
	'js',
	'mjs',
	'cjs',
	'jsx'
]);
add('json', () => import('highlight.js/lib/languages/json'), ['json']);
add('kotlin', () => import('highlight.js/lib/languages/kotlin'), ['kt', 'kts']);
add('lua', () => import('highlight.js/lib/languages/lua'), ['lua']);
add('markdown', () => import('highlight.js/lib/languages/markdown'), MARKDOWN_EXTENSIONS);
add('perl', () => import('highlight.js/lib/languages/perl'), ['pl']);
add('php', () => import('highlight.js/lib/languages/php'), ['php']);
add('powershell', () => import('highlight.js/lib/languages/powershell'), ['ps1', 'psm1']);
add('properties', () => import('highlight.js/lib/languages/properties'), ['properties']);
add('python', () => import('highlight.js/lib/languages/python'), ['py']);
add('ruby', () => import('highlight.js/lib/languages/ruby'), ['rb']);
add('rust', () => import('highlight.js/lib/languages/rust'), ['rs']);
add('scss', () => import('highlight.js/lib/languages/scss'), ['scss']);
add('sql', () => import('highlight.js/lib/languages/sql'), ['sql']);
add('swift', () => import('highlight.js/lib/languages/swift'), ['swift']);
add('typescript', () => import('highlight.js/lib/languages/typescript'), [
	'ts',
	'mts',
	'cts',
	'tsx'
]);
add('vbnet', () => import('highlight.js/lib/languages/vbnet'), ['vb']);
add('xml', () => import('highlight.js/lib/languages/xml'), [
	'xml',
	'html',
	'htm',
	'xhtml',
	'svg',
	'plist'
]);
add('yaml', () => import('highlight.js/lib/languages/yaml'), ['yml', 'yaml']);

/** 色を付ける・Markdown を組むテキストの上限 (UTF-16 の文字数。→ docs/ui.md「PDF・動画・テキストのビューアー」)。 */
export const MAX_FORMATTED_LENGTH = 256 * 1024;

/** 色を付けるファイルか。拡張子で決める (→ docs/ui.md「PDF・動画・テキストのビューアー」)。 */
export function isCodeFileName(fileName: string): boolean {
	return languageOf(fileName) !== undefined;
}

function languageOf(fileName: string) {
	const extension = fileExtension(fileName);
	return extension === undefined ? undefined : LANGUAGES[extension];
}

let core: Promise<HLJSApi> | undefined;

/**
 * `text` に色を付けた HTML。highlight.js は中身の `<`・`&` をエスケープして返すので、そのまま差し込める。
 * 色を付けないもの (言語が決まらない・長すぎる) は `undefined`。
 */
export async function highlightCode(fileName: string, text: string): Promise<string | undefined> {
	return highlight(languageOf(fileName), text);
}

/** `highlightCode` の、言語を Markdown のコードの塊の言語の指定 (`yaml`・`ts` など) で渡す版。 */
export async function highlightCodeBlock(
	language: string,
	text: string
): Promise<string | undefined> {
	const key = language.toLowerCase();
	return highlight(NAMES[key] ?? LANGUAGES[key], text);
}

async function highlight(
	language: [string, () => Promise<LanguageModule>] | undefined,
	text: string
): Promise<string | undefined> {
	if (!language || text.length > MAX_FORMATTED_LENGTH) return undefined;
	const [name, load] = language;
	core ??= import('highlight.js/lib/core').then((module) => module.default);
	const [hljs, definition] = await Promise.all([core, load()]);
	if (!hljs.getLanguage(name)) hljs.registerLanguage(name, definition.default);
	return hljs.highlight(text, { language: name, ignoreIllegals: true }).value;
}
