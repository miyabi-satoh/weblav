import * as m from '$lib/paraglide/messages.js';
import { formatList } from '$lib/format';
import { rootLocationLabel } from '$lib/root-location';
import type { components } from '$lib/api/schema';

type Operation = components['schemas']['OperationResponse'];
type Op = components['schemas']['Op'];
type OpCategory = components['schemas']['OpCategory'];

/** 絞り込みに並べる分類の順。画面の管理のタブと同じ並び。 */
export const OP_CATEGORIES: OpCategory[] = ['auth', 'contents', 'users', 'shares', 'settings'];

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

/** 変えた項目の名前 (API の項目名) を、画面の項目名で並べる。知らない名前はそのまま出す。 */
export function opFieldsLabel(fields: string[]): string {
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
	return m.admin_operations_fields({
		fields: formatList(fields.map((field) => labels[field]?.() ?? field))
	});
}
