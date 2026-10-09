/** ブラウザーのタブ・履歴・ブックマークに出る名前。 */
const APP_NAME = 'WebLAV';

/**
 * `<title>` に入れる文字列を組み立てる。
 *
 * 画面名を省くとアプリ名だけになる (トップ用)。どの画面でもアプリ名を後ろに残すのは、
 * タブを何枚も開いたときに WebLAV のものだと分かるようにするため。
 */
export function pageTitle(name?: string): string {
	return name === undefined || name === '' ? APP_NAME : `${name} - ${APP_NAME}`;
}
