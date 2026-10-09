import type { components } from '$lib/api/schema';

type LinkPreview = components['schemas']['LinkPreview'];

/** 詳しい表示に出すリンク。題は行に出しているものをそのまま渡す。 */
export type LinkDetail = {
	href: string;
	title: string;
	/** 登録した人が書いた説明。配列なら「·」で区切って並べる。 */
	description?: string | string[] | null;
	preview?: LinkPreview | null;
};

let open = $state(false);
let current = $state.raw<LinkDetail | null>(null);

/**
 * リンクの詳しい表示 (→ docs/ui.md「リンクのカード」)。一覧の行ごとにダイアログを持たず、レイアウトの1つを使い回す。
 */
export const linkDetail = {
	get open() {
		return open;
	},
	set open(value: boolean) {
		open = value;
	},
	/** 閉じるアニメーションの間も中身を残すため、閉じても消さない。 */
	get current() {
		return current;
	},
	show(detail: LinkDetail) {
		current = detail;
		open = true;
	}
};
