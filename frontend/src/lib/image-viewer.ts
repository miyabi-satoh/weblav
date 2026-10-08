/**
 * ページ内で画像を重ねて表示するビューア (→ docs/ui.md「画像のプレビュー」)。
 *
 * 拡大・スワイプ・キーボード操作・開閉のアニメーションは PhotoSwipe に任せる。
 * 画像を開くまで読み込まないよう、本体と CSS は開くときに取りに行く。
 */

import * as m from '$lib/paraglide/messages.js';
import type { ViewerItem } from '$lib/viewer-items';

export type ViewerImage = {
	/** 元の画像 (ダウンロードの URL)。 */
	src: string;
	/** 行に出す縮小画像。開く前の仮の表示と、開閉のアニメーションの起点に使う。 */
	thumbnailSrc: string;
	/** 表示するときの大きさ (px)。PhotoSwipe は開く前に大きさを知っている必要がある。 */
	width: number;
	height: number;
	title: string;
};

/** 行の縮小画像に付ける属性。開閉のアニメーションの起点を、元の画像の URL で探す。 */
export const THUMBNAIL_FOR_ATTRIBUTE = 'data-thumbnail-for';

/** 上部のバーの高さ (PhotoSwipe の `.pswp__top-bar`)。 */
const TOP_BAR_HEIGHT = 60;

/**
 * 矢印の枠は 60px。画像の左右で目に付くよう、ほかのボタンの絵の 2 倍 (48px) で描く
 * (矢印の線は 12×24px になり、PhotoSwipe の既定の矢印に近い大きさ)。
 * 線は細めて、見た目の太さをほかのボタン (2px) と揃える。
 */
const ARROW = { viewBox: '-3 -3 30 30', strokeWidth: 2 / 2 };

/**
 * PhotoSwipe のボタンの絵。lucide と同じ線の絵を、ボタンの枠 (32px) の中に 24px で描く。
 * `outlined` は地の無い画像の上に置く絵 (矢印) で、明るい画像でも見えるよう、同じ線を太く暗く下に敷く
 * (PhotoSwipe の既定の絵の `pswp__icn-shadow` と同じやり方)。
 */
function icon(
	paths: string,
	{ viewBox = '-4 -4 32 32', strokeWidth = 2, outlined = false } = {}
): string {
	const shadow = outlined
		? `<g class="pswp__icn-shadow" style="stroke-width:${strokeWidth * 2}">${paths}</g>`
		: '';
	return `<svg class="pswp__icn" viewBox="${viewBox}" aria-hidden="true" style="fill:none;stroke-linecap:round;stroke-linejoin:round">${shadow}<g style="stroke:var(--pswp-icon-color);stroke-width:${strokeWidth}">${paths}</g></svg>`;
}

/** 修飾キー付きのクリック (新しいタブ・ウィンドウで開く) は、ブラウザに任せる。 */
export function isPlainClick(event: MouseEvent): boolean {
	return event.button === 0 && !event.metaKey && !event.ctrlKey && !event.shiftKey && !event.altKey;
}

/** 元の画像を読み終えるのを待つ上限。大きな画像や遅い回線で、押しても開かないように見えないため。 */
const PRELOAD_TIMEOUT_MS = 1000;

/**
 * 元の画像を読み込み、上限までにデコードまで済んだかを返す。
 * 上限を過ぎても読み込みは続ける (開いたビューアが同じ画像を読むので、続きをキャッシュから使える)。
 * 中断されたら読み込みもやめる。
 */
function preloadImage(src: string, signal: AbortSignal): Promise<boolean> {
	const image = new Image();
	image.src = src;
	signal.addEventListener('abort', () => image.removeAttribute('src'), { once: true });
	let timer: ReturnType<typeof setTimeout> | undefined;
	return Promise.race([
		image.decode().then(
			() => true,
			() => false
		),
		new Promise<boolean>((resolve) => {
			timer = setTimeout(() => resolve(false), PRELOAD_TIMEOUT_MS);
		})
	]).finally(() => clearTimeout(timer));
}

export type OpenImageViewerOptions = {
	/** 開閉のアニメーション。ファイルのビューアから移ってきたときは付けない (行の縮小画像は後ろに隠れている)。 */
	animate?: boolean;
	/** 開く直前 (`PhotoSwipe` を作る前) に呼ぶ。 */
	beforeOpen?: () => void;
	/** 画像の並びの端から、隣のファイル (`items[target]`) へ移るときに呼ぶ。呼んだ後、このビューアはすぐに消える。 */
	onLeave: (target: number) => void;
};

/**
 * `items[index]` (画像) を開く。前後は `items` のうち、`index` を含む画像の連なり。
 * 連なりの端で前・次を押すと、隣のファイルへ `onLeave` で渡す (→ $lib/viewer-items.ts)。
 *
 * 元の画像を読み終えてから開く (上限を過ぎたら縮小画像で開く)。開き始めたら解決する。
 * 待つ間に行が消えたら (ページを移ったなど) `signal` を中断し、開かないようにする。
 */
export async function openImageViewer(
	items: ViewerItem[],
	index: number,
	signal: AbortSignal,
	{ animate = true, beforeOpen, onLeave }: OpenImageViewerOptions
): Promise<void> {
	// PhotoSwipe には、隣り合う画像の連なりだけを渡す (→ docs/ui.md「PDF・動画・テキストのビューア」)。
	let start = index;
	while (start > 0 && items[start - 1].type === 'image') start -= 1;
	let end = index;
	while (end < items.length - 1 && items[end + 1].type === 'image') end += 1;
	const images = items
		.slice(start, end + 1)
		.flatMap((item) => (item.type === 'image' ? [item.image] : []));
	const at = index - start;

	const [{ default: PhotoSwipe }, , preloaded] = await Promise.all([
		import('photoswipe'),
		import('photoswipe/style.css'),
		preloadImage(images[at].src, signal)
	]);
	if (signal.aborted) return;

	// FIX: PhotoSwipe は作った時点のフォーカスを覚え、閉じたらそこへ戻す。ファイルのビューアから移ってきたときは、
	// 消えるビューアのボタンを覚えないよう、先に閉じ切っておく。
	beforeOpen?.();
	const viewer = new PhotoSwipe({
		dataSource: images.map((image, i) => ({
			src: image.src,
			width: image.width,
			height: image.height,
			// PhotoSwipe は開くアニメーションが終わるまで元の画像を出さず、仮の表示 (msrc) を
			// 引き伸ばして見せる。読み終えていれば仮の表示にも元の画像を使い、粗い縮小画像から
			// 切り替わって見えないようにする。仮の表示を使うのは開いた1枚目だけ。
			msrc: i === at && preloaded ? image.src : image.thumbnailSrc,
			alt: image.title,
			// 行の縮小画像は中央を正方形に切り抜いてあるので、切り抜きを戻しながら開く。
			thumbCropped: true
		})),
		index: at,
		// 連なりの端は、ループせずに隣のファイルへ渡す。
		loop: false,
		showHideAnimationType: animate ? 'zoom' : 'none',
		// 何件目かは、ファイルも含めた並びで数える (下の `position`)。
		counter: false,
		// 後ろの画面を透かさない。既定の 0.8 では、ヘッダーの文字が上部のバーの数字や題名と重なる。
		bgOpacity: 1,
		// 上部のバーの下から画像を置く。バーに地を敷くので、画像の上端が隠れないように。
		padding: { top: TOP_BAR_HEIGHT, bottom: 0, left: 0, right: 0 },
		// ボタンの絵は PDF のビューア (lucide の線の絵) と揃える (→ docs/ui.md「PDF・動画・テキストのビューア」)。
		closeSVG: icon('<path d="M18 6 6 18"/><path d="m6 6 12 12"/>'),
		zoomSVG: icon(
			'<circle cx="11" cy="11" r="8"/><path d="m21 21-4.3-4.3"/><path d="M8 11h6"/><path class="pswp__zoom-icn-bar-v" d="M11 8v6"/>'
		),
		// 次へは PhotoSwipe が左右を反転させて使う。
		arrowPrevSVG: icon('<path d="m15 18-6-6 6-6"/>', { ...ARROW, outlined: true }),
		arrowNextSVG: icon('<path d="m15 18-6-6 6-6"/>', { ...ARROW, outlined: true }),
		closeTitle: m.action_close(),
		zoomTitle: m.image_viewer_zoom_button(),
		arrowPrevTitle: m.image_viewer_previous_button(),
		arrowNextTitle: m.image_viewer_next_button(),
		errorMsg: m.image_viewer_error()
	});

	viewer.addFilter('thumbEl', (thumbnail, data) => {
		const rowThumbnail = document.querySelector<HTMLElement>(
			`img[${THUMBNAIL_FOR_ATTRIBUTE}="${CSS.escape(data.src ?? '')}"]`
		);
		// 型は HTMLElement を返す形だが、PhotoSwipe は見つからない (縮小画像を読めず
		// アイコンに戻した行) ときの null も受け、拡大の代わりにフェードで開閉する。
		return (rowThumbnail ?? thumbnail) as HTMLElement;
	});

	/** 連なりの端から、隣のファイルへ。ファイルのビューアを先に開いてから消し、間にページを見せない。 */
	const leaveTo = (target: number) => {
		// FIX: PhotoSwipe は開くアニメーションの間は閉じられず (`close()` が何もしない)、PhotoSwipe が画面に残ってしまう。
		if (viewer.opener.isOpening) return;
		onLeave(target);
		viewer.destroy();
	};
	const hasPreviousFile = start > 0;
	const hasNextFile = end < items.length - 1;
	const atFirst = () => viewer.currIndex === 0;
	const atLast = () => viewer.currIndex === images.length - 1;

	viewer.on('keydown', (event) => {
		const { key } = event.originalEvent;
		if (key === 'ArrowLeft' && hasPreviousFile && atFirst()) leaveTo(start - 1);
		else if (key === 'ArrowRight' && hasNextFile && atLast()) leaveTo(end + 1);
		else return;
		event.preventDefault();
	});

	viewer.on('uiRegister', () => {
		const ui = viewer.ui;
		if (!ui) return;
		ui.registerElement({
			name: 'position',
			// PhotoSwipe の数字 (`.pswp__counter`) と同じ見た目。画像が1枚でも、並びにファイルがあれば出す。
			className: 'pswp__counter pswp__counter--items',
			appendTo: 'bar',
			order: 5,
			onInit: (element) => {
				viewer.on('change', () => {
					element.textContent =
						items.length > 1 ? `${start + viewer.currIndex + 1} / ${items.length}` : '';
				});
			}
		});
		// 連なりの端の、隣のファイルへ移る矢印。PhotoSwipe の矢印は、ループしないと端で消える。
		// タッチの端末では PhotoSwipe の矢印は出ない (スワイプで移る) が、ファイルへはスワイプで移れないので、これは出す。
		for (const [name, forward] of [
			['leavePrevious', false],
			['leaveNext', true]
		] as const) {
			ui.registerElement({
				name,
				className: `pswp__button--arrow pswp__button--leave pswp__button--arrow--${forward ? 'next' : 'prev'}`,
				title: forward ? m.file_viewer_next() : m.file_viewer_previous(),
				isButton: true,
				appendTo: 'wrapper',
				html: icon('<path d="m15 18-6-6 6-6"/>', { ...ARROW, outlined: true }),
				order: forward ? 11 : 10,
				onClick: () => leaveTo(forward ? end + 1 : start - 1),
				onInit: (element) => {
					viewer.on('change', () => {
						element.hidden = forward ? !(hasNextFile && atLast()) : !(hasPreviousFile && atFirst());
					});
				}
			});
		}
	});

	// 何の画像かを上部のバーに出す。PhotoSwipe は題名を出す部品を持たない。
	viewer.on('uiRegister', () => {
		viewer.ui?.registerElement({
			name: 'title',
			// バーの縦の中央 (数字・ボタンの中心) に揃え、残りの幅を取ってボタンを右へ寄せる。
			// 文字色は PhotoSwipe の数字 (`.pswp__counter`) に合わせる。
			className: 'min-w-0 flex-1 self-center truncate px-4 text-sm text-white',
			appendTo: 'bar',
			// 数字 (5) のすぐ後ろ。読み込み中の印 (7) は題名の後ろ、ボタンの手前に来る。
			order: 6,
			onInit: (element) => {
				viewer.on('change', () => {
					element.textContent = viewer.currSlide?.data.alt ?? '';
				});
			}
		});
	});

	viewer.init();
}
