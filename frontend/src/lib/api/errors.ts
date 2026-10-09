/**
 * backend のエラー envelope `{ error: { code, message, detail? } }` を表示文言に変換する。
 *
 * `message` は英語のデバッグ用の文字列なので表示しない。表示文言はParaglideメッセージ
 * (`messages/{locale}.json`)で持つ。`code` だけでは「入力内容を確認してください」までしか
 * 言えないため、値を見せないと直しようがない422には `detail` が付く。こちらは
 * `kind` で場合分けし、サーバーが渡した値(タイトル・軸名・公開範囲)を差し込んで組み立てる
 * (→ `src/api/error_detail.rs`)。
 */
import * as m from '$lib/paraglide/messages.js';
import type { components } from '$lib/api/schema';
import { contentTypeLabel } from '$lib/content-labels';
import { roleLabel } from '$lib/role-labels';

export type ErrorCode = components['schemas']['ErrorCode'];

export const MESSAGES: Record<ErrorCode, () => string> = {
	not_found: m.error_not_found,
	unauthorized: m.error_unauthorized,
	forbidden: m.error_forbidden,
	invalid_credentials: m.error_invalid_credentials,
	incorrect_current_password: m.error_incorrect_current_password,
	invalid_recovery_code: m.error_invalid_recovery_code,
	too_many_requests: m.error_too_many_requests,
	invalid_request_body: m.error_invalid_request_body,
	file_too_large: m.error_file_too_large,
	database_unavailable: m.error_database_unavailable,
	internal_error: m.error_internal_error,
	conflict: m.error_conflict,
	config_unreadable: m.error_config_unreadable,
	free_limit_reached: m.error_free_limit_reached,
	account_server_unavailable: m.error_account_server_unavailable,
	pro_code_invalid: m.error_pro_code_invalid,
	pro_clock_behind: m.error_pro_clock_behind
};

/** 想定外の形のレスポンスや通信エラーに使う汎用文言。 */
export const GENERIC_ERROR_MESSAGE = m.error_generic;

/**
 * `code` の文字列から表示文言を引く。
 *
 * envelope ではなく `code` だけが手元にある経路のためにある。ブラウザーが直接開いた
 * ファイルの取得に失敗すると、サーバーは `/?error=<code>` へ送ってくる
 * (→ `src/api/browser.rs`)。
 * 知らない値は汎用文言に落とす。URL は利用者が書き換えられるため。
 */
export function messageForCode(code: string | null): string {
	return isErrorCode(code) ? MESSAGES[code]() : GENERIC_ERROR_MESSAGE();
}

function isErrorCode(value: unknown): value is ErrorCode {
	return typeof value === 'string' && Object.hasOwn(MESSAGES, value);
}

/**
 * openapi-fetch の `error` (パース済み body) から envelope の中身を取り出す。
 * 本文なしの 5xx では `undefined`、text/plain 応答では文字列になることがあるため、
 * 型に頼らず実行時に形を確認する。
 */
function unwrapError(body: unknown): { code?: unknown; detail?: unknown } | undefined {
	if (typeof body !== 'object' || body === null) return undefined;
	const error = (body as { error?: unknown }).error;
	if (typeof error !== 'object' || error === null) return undefined;
	return error as { code?: unknown; detail?: unknown };
}

/** envelope から `code` を取り出す。 */
export function errorCode(body: unknown): ErrorCode | undefined {
	const code = unwrapError(body)?.code;
	return isErrorCode(code) ? code : undefined;
}

type ValidationDetail = components['schemas']['ValidationDetail'];
type FreeLimitTarget = components['schemas']['FreeLimitTarget'];

/** Free の上限のある対象の呼び名。一覧や作成の画面と同じ語を使う。 */
export function freeLimitTargetLabel(target: FreeLimitTarget): string {
	return target === 'admin' || target === 'user' ? roleLabel(target) : contentTypeLabel(target);
}

function freeLimitMessage(target: FreeLimitTarget, limit: number): string {
	const label = freeLimitTargetLabel(target);
	return target === 'admin' || target === 'user'
		? m.error_detail_free_limit_role({ role: label, limit })
		: m.error_detail_free_limit_content({ kind: label, limit });
}

/**
 * 422 (と Free の上限の 409) の内訳から表示文言を組み立てる。未知の `kind` (サーバーだけ先に更新された場合)
 * では `undefined` を返し、呼び出し側が `code` の汎用文言に落とす。
 */
function detailMessage(detail: ValidationDetail): string | undefined {
	switch (detail.kind) {
		case 'groupCannotBePrivate':
			return m.contents_form_visibility_group_private_error();
		case 'axisNameHasBraces':
			return m.error_detail_axis_name_braces();
		case 'axisNameTaken':
			return m.error_detail_axis_name_taken({ name: detail.name });
		case 'axisNameReserved':
			return m.error_detail_axis_name_reserved({ name: detail.name });
		case 'importAxisInvalid':
			return m.error_detail_import_axis_invalid({ name: detail.name });
		case 'templateUnknownAxis':
			return m.error_detail_template_unknown_axis({ name: detail.name });
		case 'axisInUseByTemplate':
			return m.error_detail_axis_in_use_by_template({ name: detail.name });
		case 'usernameTaken':
			return m.error_detail_username_taken({ name: detail.name });
		case 'lastAdmin':
			return m.error_detail_last_admin();
		case 'rootNameTaken':
			return m.error_detail_root_name_taken({ name: detail.name });
		case 'rootAlreadyRegistered':
			return m.error_detail_root_already_registered({ name: detail.name });
		case 'rootInsideOwnDirs':
			return m.error_detail_root_inside_own_dirs();
		case 'archiveFolderMissing':
			return m.error_detail_archive_folder_missing();
		case 'archiveFolderUnreadable':
			return unreadableFolderMessage(detail.path);
		case 'backupInvalid':
			return m.error_detail_backup_invalid();
		case 'backupTooNew':
			return m.error_detail_backup_too_new({ appVersion: detail.appVersion });
		case 'freeLimit':
			return freeLimitMessage(detail.target, detail.limit);
		default:
			return undefined;
	}
}

/** 読めなかった場所の文言。登録前の確認でも出す。空文字は登録先そのもの (→ `src/api/error_detail.rs`)。 */
export function unreadableFolderMessage(path: string): string {
	return path === ''
		? m.error_detail_archive_folder_unreadable_root()
		: m.error_detail_archive_folder_unreadable({ path });
}

/** envelope から `detail` を取り出す。 */
export function errorDetail(body: unknown): ValidationDetail | undefined {
	const detail = unwrapError(body)?.detail;
	if (typeof detail !== 'object' || detail === null) return undefined;
	if (typeof (detail as { kind?: unknown }).kind !== 'string') return undefined;
	return detail as ValidationDetail;
}

/** 表示文言を返す。`detail` があればそれを優先し、無ければ `code`、不明なら汎用文言。 */
export function errorMessage(body: unknown): string {
	const detail = errorDetail(body);
	if (detail) {
		const message = detailMessage(detail);
		if (message !== undefined) return message;
	}
	const code = errorCode(body);
	return code ? MESSAGES[code]() : GENERIC_ERROR_MESSAGE();
}
