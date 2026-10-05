import { getLocale } from '$lib/paraglide/runtime';

/** バイト数を読みやすい単位(B/KB/MB/GB/TB)に変換する。`null`/`undefined` は空文字にする。
 * 数と単位の間で折り返さないよう、ノーブレークスペースで区切る (→ docs/ui.md「UI 全般」)。 */
export function formatByteSize(size: number | null | undefined): string {
	if (size == null) return '';
	if (size < 1024) return `${size}\u00a0B`;
	const units = ['KB', 'MB', 'GB', 'TB'];
	let value = size / 1024;
	let unitIndex = 0;
	while (value >= 1024 && unitIndex < units.length - 1) {
		value /= 1024;
		unitIndex += 1;
	}
	return `${value.toFixed(1)}\u00a0${units[unitIndex]}`;
}

/** 日時を画面の表示言語 (→ docs/ui.md「UI 全般」) の表示形式に変換する。`null`/`undefined` は空文字にする。 */
export function formatDateTime(value: number | string | null | undefined): string {
	if (value == null) return '';
	return new Date(value).toLocaleString(getLocale());
}

/** 日付だけを画面の表示言語の表示形式に変換する。`null`/`undefined` は空文字にする。 */
export function formatDate(value: number | string | null | undefined): string {
	if (value == null) return '';
	return new Date(value).toLocaleDateString(getLocale());
}

/** 数値を画面の表示言語の桁区切りで書く。`null`/`undefined` は空文字にする。 */
export function formatNumber(value: number | null | undefined): string {
	if (value == null) return '';
	return value.toLocaleString(getLocale());
}

/** 再生位置・長さを `m:ss` (1時間以上は `h:mm:ss`) で書く。長さが分からない値は `0:00` にする。 */
export function formatDuration(seconds: number): string {
	const total = Number.isFinite(seconds) && seconds > 0 ? Math.floor(seconds) : 0;
	const h = Math.floor(total / 3600);
	const m = Math.floor((total % 3600) / 60);
	const s = String(total % 60).padStart(2, '0');
	return h > 0 ? `${h}:${String(m).padStart(2, '0')}:${s}` : `${m}:${s}`;
}

/** 相対の経過を書く単位。大きい方から当てはめる。 */
const RELATIVE_UNITS: [Intl.RelativeTimeFormatUnit, number][] = [
	['year', 365 * 24 * 60 * 60],
	['month', 30 * 24 * 60 * 60],
	['day', 24 * 60 * 60],
	['hour', 60 * 60],
	['minute', 60]
];

/**
 * 日時から `now` までの経過を、画面の表示言語で「3時間前」のように書く。読めない日時は空文字にする。
 * 1分に満たないもの・先の日時は「たった今」にまとめる (リンクの公開日時は相手の時計しだいで、少し先のこともあるため)。
 */
export function formatTimeAgo(value: string | null | undefined, now: number = Date.now()): string {
	if (value == null) return '';
	const time = Date.parse(value);
	if (Number.isNaN(time)) return '';
	const elapsed = Math.max(0, Math.floor((now - time) / 1000));
	const format = new Intl.RelativeTimeFormat(getLocale(), { numeric: 'auto' });
	for (const [unit, seconds] of RELATIVE_UNITS) {
		if (elapsed >= seconds) return format.format(-Math.floor(elapsed / seconds), unit);
	}
	return format.format(0, 'second');
}
