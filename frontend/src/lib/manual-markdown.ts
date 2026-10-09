import { marked, Renderer, type Tokens } from 'marked';
import { helpImageHref } from '$lib/api/urls';
import { escapeHtml } from '$lib/html';

/**
 * 横に送れる表を、キーボードでも操作できる領域で包む renderer。
 * マニュアルと、テキストのビューアーの Markdown のプレビュー (→ $lib/markdown-preview.ts) で使う。
 *
 * 表そのものを `overflow` のコンテナにすると、表の中に押せる要素が無い状態では
 * フォーカスが当たらず、見切れた列にキーボードだけで到達できない
 * (Chrome は補うが Safari と Firefox は補わない → docs/ui.md「UI 全般」)。
 * `display` を変えずに済むので、支援技術から表の役割が外れる心配も無くなる。
 */
export class ScrollTableRenderer extends Renderer {
	// `erasableSyntaxOnly` が有効なのでコンストラクタの引数プロパティは使えない。
	readonly tableLabel: string;

	constructor(tableLabel: string) {
		super();
		this.tableLabel = tableLabel;
	}

	override table(token: Tokens.Table): string {
		const label = escapeHtml(this.tableLabel);
		return `<div class="table-scroll" tabindex="0" role="region" aria-label="${label}">${super.table(token)}</div>`;
	}
}

class ManualRenderer extends ScrollTableRenderer {
	readonly locale: string;

	constructor(tableLabel: string, locale: string) {
		super(tableLabel);
		this.locale = locale;
	}

	/**
	 * 他のページへのリンクは、原典のファイル名 (`02-setup.md`) で書く。
	 * リポジトリ上でも辿れるため (→ docs/help.md)。
	 */
	override link(token: Tokens.Link): string {
		const page = /^\d+-([a-z0-9-]+)\.md(#.*)?$/.exec(token.href);
		if (!page) return super.link(token);
		return super.link({ ...token, href: `/help/${page[1]}${page[2] ?? ''}` });
	}

	/**
	 * 画像は `images/<名前>.webp` で書く。画面の文言が写っているので、本文と同じ言語のものを
	 * 取りに行く (訳の無い画像はサーバーが原典に落とす → docs/help.md)。
	 * 画像は2倍の解像度で撮ってあるので、`2x` と宣言して実物と同じ大きさで出す。
	 * `src` を残すと同じ画像が 1x の候補にもなり、等倍の画面ではそちらが選ばれるため、`srcset` だけにする。
	 */
	override image(token: Tokens.Image): string {
		const image = /^images\/([a-z0-9-]+\.webp)$/.exec(token.href);
		if (!image) return super.image(token);
		const href = helpImageHref(image[1], this.locale);
		return super
			.image({ ...token, href })
			.replace(/^<img src="([^"]*)"/, '<img loading="lazy" srcset="$1 2x"');
	}
}

const CJK = '[　-ヿ一-鿿＀-￯]';
/** 直後に空白を置かない約物。これで終わる行は、次に何が来ても詰めてよい。 */
const TERMINATOR = '[。、！？」』）]';
const CLOSING_TAGS = '(?:</[a-zA-Z][^>]*>)*';
const OPENING_TAGS = '(?:<[a-zA-Z][^>]*>)*';

// 次の文字は先読みで見る。消費すると `あ\nい\nう` の2つ目の改行を取りこぼすため。
const AFTER_TERMINATOR = new RegExp(`(${TERMINATOR}${CLOSING_TAGS})\\n`, 'g');
const BETWEEN_CJK = new RegExp(`(${CJK}${CLOSING_TAGS})\\n(?=${OPENING_TAGS}${CJK})`, 'g');

/**
 * 日本語の行送りが半角空白になって出るのを防ぐ。
 *
 * Markdown は段落の中の改行を空白として繋ぐ。英語はそれで正しいが、日本語は語の間に
 * 空白を置かないので、文の区切りや途中に隙間が開いて見える。
 *
 * 詰めるのは次の2つ。`<code>` や `<strong>` が境目に挟まっても効く。
 *
 * - 約物 (`。` など) の直後の改行。日本語では次が欧文でも空白を置かない
 * - 両側が日本語の文字である改行
 *
 * 逆に、日本語と欧文の境目 (`環境変数` と `WEBLAV_HOME` の間など) の空白は残す。
 * `<pre>` の中は行が意味を持つので触らない。
 */
export function collapseCjkLineBreaks(html: string): string {
	return html
		.split(/(<pre[\s\S]*?<\/pre>)/)
		.map((part, index) =>
			index % 2 === 1 ? part : part.replace(AFTER_TERMINATOR, '$1').replace(BETWEEN_CJK, '$1')
		)
		.join('');
}

/**
 * マニュアルの Markdown を HTML にする。
 *
 * ソースは自分たちで書く `docs/manual/{ロケール}/*.md` だけなので、外部入力の
 * サニタイズ問題は生じない (→ docs/help.md)。DOMPurify 等は挟まない。
 *
 * @param tableLabel 表を包む領域の名前。画面の造りの一部なので、本文ではなく
 *                   画面の表示言語で渡すこと (→ docs/help.md)。
 * @param locale 本文の言語 (サーバーが実際に返したもの)。画像をこの言語で取る。
 */
export function renderManual(body: string, tableLabel: string, locale: string): string {
	const renderer = new ManualRenderer(tableLabel, locale);
	return collapseCjkLineBreaks(marked.parse(body, { async: false, renderer }));
}
