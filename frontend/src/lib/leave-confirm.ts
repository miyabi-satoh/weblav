import { goto } from '$app/navigation';

let bypassed = false;

/**
 * 編集中の離脱の確認を出さずに移る。編集していたコンテンツを消した後に一覧へ移すときに使う。
 * 消したものの変更は保存しようがないので、確かめても選べることが無いため。
 * `beforeNavigate` は `goto` の中で同期して呼ばれるので、その間だけ印を立てる。
 */
export async function gotoWithoutLeaveConfirm(url: string): Promise<void> {
	bypassed = true;
	try {
		// eslint-disable-next-line svelte/no-navigation-without-resolve -- 呼び出し側が resolve() した URL を渡す
		await goto(url);
	} finally {
		bypassed = false;
	}
}

/** 離脱の確認を出す `beforeNavigate` が、今の移動では確かめないかを見る。 */
export function leaveConfirmBypassed(): boolean {
	return bypassed;
}
