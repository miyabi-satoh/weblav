/**
 * 閲覧者として見る (→ docs/ui.md「UI 全般」)。ログインしたまま、ログインしていない人に見える形で閲覧側を出す。
 * cookie を置くと、サーバーは閲覧側の API (`Viewer`) でこの人を匿名として扱う。管理画面の API は変わらない。
 * cookie はブラウザーを閉じると消える (期限を付けない)。戻し忘れても、次に開いたときは普段の見え方に戻るように。
 * 切り替えた後は、呼び出し側が `refreshAll()` で画面を取り直す。
 */
export const VIEW_AS_COOKIE = 'WEBLAV_VIEW_AS';

/** `document.cookie` の文字列に、閲覧者として見る cookie があるか。 */
export function hasViewAsCookie(cookie: string): boolean {
	return cookie.split(';').some((part) => part.trim() === `${VIEW_AS_COOKIE}=anonymous`);
}

class ViewAsState {
	active = $state(typeof document !== 'undefined' && hasViewAsCookie(document.cookie));

	set(on: boolean) {
		document.cookie = on
			? `${VIEW_AS_COOKIE}=anonymous; path=/; SameSite=Lax`
			: `${VIEW_AS_COOKIE}=; path=/; max-age=0; SameSite=Lax`;
		this.active = on;
	}
}

export const viewAs = new ViewAsState();
