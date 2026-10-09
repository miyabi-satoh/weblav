import type { components } from '$lib/api/schema';
import { isLanUrl, urlHost } from '$lib/file-kind';

type LinkPreview = components['schemas']['LinkPreview'];

/** 詳しい表示に出すリンク。 */
export type LinkDetail = {
	href: string;
	title: string;
	/** 登録した人が書いた説明。配列なら「·」で区切って並べる。 */
	description?: string | string[] | null;
	preview?: LinkPreview | null;
};

/** リンクの行と詳しい表示に出す題。題を省くと、ページのタイトル (無ければホスト名)。 */
export function linkTitleOf(
	href: string,
	title: string | undefined,
	preview: LinkPreview | null | undefined
): string {
	return title ?? preview?.title ?? urlHost(href);
}

/** 行に出すのと同じ題 (`linkTitleOf`) で、詳しい表示に出すリンクを組む。 */
export function linkDetailOf(link: {
	href: string;
	title?: string;
	description?: string | string[] | null;
	preview?: LinkPreview | null;
}): LinkDetail {
	return {
		href: link.href,
		title: linkTitleOf(link.href, link.title, link.preview),
		description: link.description,
		preview: link.preview
	};
}

/**
 * 押すと詳しい表示を出すリンクか。ビューアーで開くもの (動画サイト・URL のファイル) はビューアーで開き、
 * LAN の URL はサーバーが取りに行かず見せるものが無いので、直接開く (→ docs/ui.md「リンクのカード」)。
 */
export function opensLinkDetail(href: string, opensInViewer: boolean): boolean {
	return !opensInViewer && !isLanUrl(href);
}

let open = $state(false);
let items = $state.raw<LinkDetail[]>([]);
let index = $state(0);

/**
 * リンクの詳しい表示 (→ docs/ui.md「リンクのカード」)。一覧の行ごとにダイアログを持たず、レイアウトの1つを使い回す。
 * 前後は、開いた一覧に並ぶ、詳しい表示を出すリンクの表示順。
 */
export const linkDetail = {
	get open() {
		return open;
	},
	set open(value: boolean) {
		open = value;
	},
	/** 閉じるアニメーションの間も中身を残すため、閉じても消さない。 */
	get current(): LinkDetail | null {
		return items[index] ?? null;
	},
	get index() {
		return index;
	},
	get count() {
		return items.length;
	},
	show(list: LinkDetail[], at: number) {
		items = list;
		index = at;
		open = true;
	},
	previous() {
		if (index > 0) index -= 1;
	},
	next() {
		if (index < items.length - 1) index += 1;
	}
};
