import * as m from '#lib/paraglide/messages.js';
import { axisSourceLabel } from '#lib/axis-labels.ts';
import { visibilityLabel } from '#lib/content-labels.ts';
import { formatList, formatNumber } from '#lib/format.ts';
import { roleLabel } from '#lib/role-labels.ts';
import { rootLocationLabel } from '#lib/root-location.ts';
import { shareAccessLabel } from '#lib/share-ops.ts';
import type { components } from '#lib/api/schema.js';

type Operation = components['schemas']['OperationResponse'];
type Op = components['schemas']['Op'];
type OpCategory = components['schemas']['OpCategory'];
type FieldChange = components['schemas']['FieldChange'];
type Visibility = components['schemas']['Visibility'];
type Role = components['schemas']['Role'];
type ShareAccess = components['schemas']['ShareAccess'];
type AxisSource = components['schemas']['AxisSource'];

/** 絞り込みに並べる分類の順。画面の管理のタブと同じ並び。 */
export const OP_CATEGORIES: OpCategory[] = ['auth', 'contents', 'users', 'shares', 'settings'];

/**
 * URL クエリの絞り込み。知らない値は絞り込まないものとして扱う。消したユーザーなど、
 * 一覧に無いユーザーも知らない値とする。絞り込みの欄に名前を出せず、「すべて」と出したまま絞り込むことになるため。
 */
export function opFilters(
	params: URLSearchParams,
	users: readonly { id: number }[]
): { category?: OpCategory; userId?: number } {
	const category = OP_CATEGORIES.find((value) => value === params.get('category'));
	const user = Number(params.get('user'));
	return { category, userId: users.some(({ id }) => id === user) ? user : undefined };
}

export function opCategoryLabel(category: OpCategory): string {
	switch (category) {
		case 'auth':
			return m.admin_operations_category_auth();
		case 'contents':
			return m.admin_operations_category_contents();
		case 'users':
			return m.admin_operations_category_users();
		case 'shares':
			return m.admin_operations_category_shares();
		case 'settings':
			return m.admin_operations_category_settings();
	}
}

/** 操作の記録の、操作の名前。 */
export function opLabel(op: Op): string {
	const labels: Record<Op, () => string> = {
		login: m.admin_operations_op_login,
		logout: m.admin_operations_op_logout,
		change_password: m.admin_operations_op_change_password,
		change_username: m.admin_operations_op_change_username,
		create_recovery_code: m.admin_operations_op_create_recovery_code,
		recover: m.admin_operations_op_recover,
		create_content: m.admin_operations_op_create_content,
		update_content: m.admin_operations_op_update_content,
		change_creator: m.admin_operations_op_change_creator,
		delete_content: m.admin_operations_op_delete_content,
		create_axis: m.admin_operations_op_create_axis,
		update_axis: m.admin_operations_op_update_axis,
		delete_axis: m.admin_operations_op_delete_axis,
		replace_axis_values: m.admin_operations_op_replace_axis_values,
		replace_axes: m.admin_operations_op_replace_axes,
		rescan: m.admin_operations_op_rescan,
		create_user: m.admin_operations_op_create_user,
		change_role: m.admin_operations_op_change_role,
		reset_password: m.admin_operations_op_reset_password,
		rename_user: m.admin_operations_op_rename_user,
		delete_user: m.admin_operations_op_delete_user,
		upload: m.admin_operations_op_upload,
		mkdir: m.admin_operations_op_mkdir,
		rename: m.admin_operations_op_rename,
		move: m.admin_operations_op_move,
		copy: m.admin_operations_op_copy,
		delete: m.admin_operations_op_delete,
		list_on_home: m.admin_operations_op_list_on_home,
		create_share: m.admin_operations_op_create_share,
		rename_share: m.admin_operations_op_rename_share,
		change_share_access: m.admin_operations_op_change_share_access,
		move_share: m.admin_operations_op_move_share,
		delete_share: m.admin_operations_op_delete_share,
		update_site_settings: m.admin_operations_op_update_site_settings,
		update_server_settings: m.admin_operations_op_update_server_settings,
		update_log_settings: m.admin_operations_op_update_log_settings,
		download_backup: m.admin_operations_op_download_backup,
		restore_backup: m.admin_operations_op_restore_backup
	};
	return labels[op]();
}

/** 済んだもの以外の結果の印。済んだものは印を付けない (ほとんどが済んだもので、並べると目立たなくなるため)。 */
export function opResultLabel(operation: Operation): string | undefined {
	if (operation.result === 'running') return m.admin_operations_result_running();
	if (operation.result === 'failed') return m.admin_operations_result_failed();
	if (operation.result === 'cancelled') return m.admin_operations_result_cancelled();
	return undefined;
}

/**
 * 扱ったもの。共有の中の操作は場所を並べ、ほかは対象の名前 (消した後もそのときの名前) を出す。
 * 置き先・変えた後の名前があれば `to` に書く。対象の無い操作 (ログインなど) は `undefined`。
 */
export function opPlaces(operation: Operation): { from: string; to?: string } | undefined {
	if (operation.shareName != null) {
		const from = formatList(
			operation.paths.map((path) => rootLocationLabel(operation.shareName ?? '', path))
		);
		if (operation.destShareName == null) return { from };
		return { from, to: rootLocationLabel(operation.destShareName, operation.destPath ?? '') };
	}
	if (operation.targetName == null) return undefined;
	const from =
		operation.targetPart == null
			? operation.targetName
			: m.admin_operations_target_part({
					target: operation.targetName,
					part: operation.targetPart
				});
	return operation.targetRenamedTo == null ? { from } : { from, to: operation.targetRenamedTo };
}

/** 項目の名前 (API の項目名) を画面の項目名にする。知らない名前はそのまま出す。 */
function fieldLabel(field: FieldChange): string {
	if (field.name === 'userAccess') {
		return m.admin_operations_field_user_access({ user: field.subject ?? '' });
	}
	const labels: Record<string, () => string> = {
		parentId: m.contents_form_parent_label,
		title: m.contents_form_title_label,
		url: m.contents_form_url_label,
		path: m.contents_form_path_label,
		description: m.contents_form_description_label,
		visibility: m.contents_form_visibility_label,
		includePatterns: m.contents_form_include_patterns_label,
		excludePatterns: m.contents_form_exclude_patterns_label,
		titleTemplate: m.archive_axes_template_title,
		listView: m.contents_form_list_view_label,
		autoRescan: m.contents_form_auto_rescan_label,
		createdBy: m.contents_form_creator_label,
		role: m.admin_users_role_label,
		editorAccess: m.admin_roots_access_default_label,
		name: m.archive_axes_dialog_name_label,
		source: m.archive_axes_dialog_source_label,
		dirLevel: m.archive_axes_dialog_dir_level_label,
		optionalInTitle: m.admin_operations_field_optional_in_title,
		filterable: m.archive_axes_dialog_filterable_label,
		position: m.admin_operations_field_position,
		siteName: m.admin_settings_site_name_label,
		homeHeading: m.admin_settings_home_heading_label,
		port: m.admin_settings_port_label,
		uploadMaxSizeMb: m.admin_settings_upload_label,
		sessionExpiryDays: m.admin_settings_session_label,
		verbose: m.admin_settings_log_verbose_label
	};
	return labels[field.name]?.() ?? field.name;
}

/** 項目の値を、画面で選ぶときの言葉にする。形の分からない値は、そのまま文字にする。 */
function valueLabel(name: string, value: unknown): string {
	if (value === null || value === undefined || value === '') {
		if (name === 'userAccess') return m.admin_operations_value_no_exception();
		if (name === 'parentId') return m.breadcrumb_home();
		if (name === 'uploadMaxSizeMb') return m.admin_settings_upload_unlimited();
		return m.admin_operations_value_none();
	}
	if (typeof value === 'boolean') {
		return value ? m.admin_operations_value_on() : m.admin_operations_value_off();
	}
	switch (name) {
		case 'visibility':
			return visibilityLabel(value as Visibility);
		case 'role':
			return roleLabel(value as Role);
		case 'editorAccess':
		case 'userAccess':
			return shareAccessLabel(value as ShareAccess);
		case 'source':
			return axisSourceLabel(value as AxisSource);
		case 'path': {
			const place = value as { share: string; path: string };
			return rootLocationLabel(place.share, place.path);
		}
		case 'autoRescan': {
			const schedule = value as { mode: string; hours?: number | null; time?: string | null };
			if (schedule.mode === 'interval') {
				return m.admin_operations_value_interval({ hours: schedule.hours ?? 0 });
			}
			if (schedule.mode === 'daily') {
				return m.admin_operations_value_daily({ time: schedule.time ?? '' });
			}
			return m.schedule_off();
		}
		case 'sessionExpiryDays':
			return value === 0
				? m.admin_settings_session_unlimited()
				: m.admin_operations_value_days({ days: Number(value) });
		case 'uploadMaxSizeMb':
			return m.admin_operations_value_megabytes({ size: formatNumber(Number(value)) });
	}
	return typeof value === 'object' ? JSON.stringify(value) : String(value);
}

/**
 * 編集で変えた項目を、1項目1行の文にする。前と後の値があるものは「項目: 前 → 後」、
 * 値を残さない長い文の項目は、まとめて最後の1行に名前だけを並べる。
 */
export function opFieldLines(fields: FieldChange[]): string[] {
	const lines = fields
		.filter((field) => field.values != null)
		.map((field) =>
			m.admin_operations_change({
				field: fieldLabel(field),
				before: valueLabel(field.name, field.values?.before),
				after: valueLabel(field.name, field.values?.after)
			})
		);
	const named = fields.filter((field) => field.values == null).map(fieldLabel);
	if (named.length > 0) lines.push(m.admin_operations_fields({ fields: formatList(named) }));
	return lines;
}
