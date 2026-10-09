<script lang="ts">
	import { adminPageClass } from '$lib/page-layout';
	import { tick, untrack } from 'svelte';
	import { flip } from 'svelte/animate';
	import { invalidateAll } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { client } from '$lib/api/client';
	import { archiveItemManageDownloadHref } from '$lib/api/urls';
	import { GENERIC_ERROR_MESSAGE, errorMessage } from '$lib/api/errors';
	import {
		WORD_CANDIDATES_PAGE_SIZE,
		axisValuesPreviewBlocker,
		FILENAME_WORD_VALUES_LIMIT,
		wordCandidates
	} from '$lib/axis-value-assist';
	import { formatNumber } from '$lib/format';
	import { ErrorDialogState } from '$lib/error-dialog-state.svelte';
	import { LatestRequest } from '$lib/latest-request';
	import { moveItem } from '$lib/reorder';
	import { SORTABLE_FLIP_DURATION_MS, SortableZone } from '$lib/sortable-zone.svelte';
	import { columnFromLgClass, columnFromSmClass, dragHandleClass } from '$lib/table-columns';
	import { byPosition } from '$lib/sort';
	import * as m from '$lib/paraglide/messages.js';
	import type { components } from '$lib/api/schema';
	import { Button } from '$lib/components/ui/button';
	import { Checkbox } from '$lib/components/ui/checkbox';
	import * as Dialog from '$lib/components/ui/dialog';
	import * as DropdownMenu from '$lib/components/ui/dropdown-menu';
	import * as Field from '$lib/components/ui/field';
	import * as RadioGroup from '$lib/components/ui/radio-group';
	import { Input } from '$lib/components/ui/input';
	import { Spinner } from '$lib/components/ui/spinner';
	import * as Select from '$lib/components/ui/select';
	import * as Table from '$lib/components/ui/table';
	import ConfirmDialog from '$lib/components/confirm-dialog.svelte';
	import ErrorDialog from '$lib/components/error-dialog.svelte';
	import WarningBand from '$lib/components/warning-band.svelte';
	import AxesAiDialog from '$lib/components/axes-ai-dialog.svelte';
	import LoadingButton from '$lib/components/loading-button.svelte';
	import { toast } from 'svelte-sonner';
	import ChevronUpIcon from '@lucide/svelte/icons/chevron-up';
	import ChevronDownIcon from '@lucide/svelte/icons/chevron-down';
	import GripVerticalIcon from '@lucide/svelte/icons/grip-vertical';
	import XIcon from '@lucide/svelte/icons/x';
	import type { Attachment } from 'svelte/attachments';
	import type { PageProps } from './$types';
	import { FormDirtyState } from '$lib/form-dirty.svelte';

	type Axis = components['schemas']['AxisResponse'];
	type AxisSource = components['schemas']['AxisSource'];
	type AxisMatchPosition = components['schemas']['AxisMatchPosition'];
	type AxisRequest = components['schemas']['AxisRequest'];
	type DirLevelSummary = components['schemas']['DirLevelSummary'];
	type FilenameWordsResponse = components['schemas']['FilenameWordsResponse'];
	type AxisValuesPreview = components['schemas']['AxisValuesPreviewResponse'];
	type AxisValuesPreviewRow = components['schemas']['AxisValuesPreviewRow'];

	let { data }: PageProps = $props();

	// 書き換えられない人 (他人のアーカイブを開いた `user`) には見るだけにする (→ docs/ui.md「UI 全般」)。
	let editable = $derived(data.editable);

	// サーバー応答をローカルへ反映できるよう再代入可能な $derived で持つ
	// (再代入のみで更新し、in-place mutate はしない)。
	let axes = $derived(data.axes);
	let sortedAxes = $derived([...axes].sort(byPosition));

	// 軸のリネームでサーバー側の`title_template`が書き換わりうるため、常に最新の
	// contentを保持する。テンプレート保存もこの値を土台にする。
	let currentContent = $derived(data.content);

	/** 軸ではない予約プレースホルダー。サーバーの `archive::FILE_NAME_PLACEHOLDER` と揃える。 */
	const FILE_NAME_PLACEHOLDER = 'fileName';

	const errorDialog = new ErrorDialogState();

	// 選択中の軸(idで持ち、$effectの依存を軸オブジェクトの参照ではなくidにする。
	// 軸を編集するとaxesの中身が新しい参照に置き換わるため、参照で依存すると
	// 編集のたびに値の辞書を読み直してしまう)。
	let selectedAxisId = $state<number | null>(null);
	let selectedAxis = $derived(axes.find((axis) => axis.id === selectedAxisId) ?? null);

	const MATCH_POSITIONS: AxisMatchPosition[] = ['anywhere', 'wordStart', 'end'];

	function matchPositionLabel(position: AxisMatchPosition): string {
		if (position === 'wordStart') return m.archive_axes_match_position_option_word_start();
		if (position === 'end') return m.archive_axes_match_position_option_end();
		return m.archive_axes_match_position_option_anywhere();
	}

	const AXIS_SOURCES: AxisSource[] = ['dirLevel', 'filenameWord'];

	function sourceOptionLabel(source: AxisSource): string {
		return source === 'dirLevel'
			? m.archive_axes_source_option_dir_level()
			: m.archive_axes_source_option_filename_word();
	}

	/** 軸の保存に送る body。作成・編集・並べ替えで送る項目を揃える。 */
	function axisRequestBody(axis: Omit<Axis, 'id'>): AxisRequest {
		return {
			name: axis.name,
			source: axis.source,
			dirLevel: axis.dirLevel,
			optionalInTitle: axis.optionalInTitle,
			filterable: axis.filterable,
			position: axis.position
		};
	}

	function axisSourceLabel(axis: Axis): string {
		return axis.source === 'dirLevel'
			? m.archive_axes_source_dir_level({ level: String(axis.dirLevel ?? '') })
			: m.archive_axes_source_filename_word();
	}

	// 軸の追加・編集ダイアログ。
	let axisDialogOpen = $state(false);
	let aiDialogOpen = $state(false);
	let editingAxisId = $state<number | null>(null);
	let axisFormName = $state('');
	let axisFormSource = $state<AxisSource>('dirLevel');
	let axisFormDirLevel = $state<number | undefined>(1);
	let axisFormFilterable = $state(true);
	let axisFormOptionalInTitle = $state(false);
	let axisSaving = $state(false);

	// 開いた時点の値。入力中のダイアログを外側クリックやEscで閉じてしまわないよう、
	// dirty判定に使う (コンテンツ管理のフォームと同じ扱い)。
	const axisForm = new FormDirtyState(() => ({
		name: axisFormName,
		source: axisFormSource,
		dirLevel: axisFormDirLevel,
		filterable: axisFormFilterable,
		optionalInTitle: axisFormOptionalInTitle
	}));

	let editingAxis = $derived(axes.find((axis) => axis.id === editingAxisId) ?? null);

	// 編集中に抽出元か階層番号を変えると、保存でサーバーが値の辞書を消す (→ docs/archive.md「軸の値の辞書と導出」)。
	let axisValuesWillBeCleared = $derived.by(() => {
		if (!editingAxis) return false;
		if (axisFormSource !== editingAxis.source) return true;
		return axisFormSource === 'dirLevel' && axisFormDirLevel !== editingAxis.dirLevel;
	});

	// 階層ごとの値 (→ docs/archive.md「軸の設定を助ける表示」)。保存・再スキャンで変わるので、ダイアログを開くたびに取り直す。
	// 取れなかったとき・アイテムが無いときは、階層番号の入力欄に戻す。
	type DirLevelsState =
		{ status: 'loading' } | { status: 'loaded'; levels: DirLevelSummary[] } | { status: 'failed' };

	let dirLevels = $state<DirLevelsState>({ status: 'loading' });
	const dirLevelsRequests = new LatestRequest();

	async function loadDirLevels() {
		const isCurrent = dirLevelsRequests.begin();
		dirLevels = { status: 'loading' };
		try {
			const { data: levels, response } = await client.GET('/api/v1/contents/{id}/dir-levels', {
				params: { path: { id: data.contentId } }
			});
			if (!isCurrent()) return;
			dirLevels = response.ok && levels ? { status: 'loaded', levels } : { status: 'failed' };
		} catch {
			if (isCurrent()) dirLevels = { status: 'failed' };
		}
	}

	type DirLevelOption = { level: number; summary: DirLevelSummary | null };

	// 選択肢。編集中の軸の階層が一覧に無ければ (アイテムが変わった等)、選んだ状態を見せるために足す。
	let dirLevelOptions = $derived.by((): DirLevelOption[] => {
		if (dirLevels.status !== 'loaded' || dirLevels.levels.length === 0) return [];
		const options: DirLevelOption[] = dirLevels.levels.map((summary) => ({
			level: summary.level,
			summary
		}));
		const savedLevel = editingAxis?.source === 'dirLevel' ? editingAxis.dirLevel : null;
		if (
			savedLevel !== null &&
			savedLevel !== undefined &&
			!options.some((option) => option.level === savedLevel)
		) {
			options.push({ level: savedLevel, summary: null });
			options.sort((a, b) => a.level - b.level);
		}
		return options;
	});

	function dirLevelSamplesText(summary: DirLevelSummary): string {
		const samples = summary.samples
			.map((sample) => sample.value)
			.join(m.archive_axes_dialog_dir_level_samples_separator());
		const params = { samples, count: String(summary.valueCount) };
		return summary.valueCount > summary.samples.length
			? m.archive_axes_dialog_dir_level_option_samples_more(params)
			: m.archive_axes_dialog_dir_level_option_samples(params);
	}

	// 階層を読み込み中の保存は、選択肢を見ないまま既定の階層で作ってしまうので止める。
	let dirLevelsPending = $derived(axisFormSource === 'dirLevel' && dirLevels.status === 'loading');

	function openCreateAxisDialog() {
		editingAxisId = null;
		axisFormName = '';
		axisFormSource = 'dirLevel';
		axisFormDirLevel = 1;
		axisFormFilterable = true;
		axisFormOptionalInTitle = false;
		axisForm.markPristine();
		axisDialogOpen = true;
		loadDirLevels();
	}

	function openEditAxisDialog(axis: Axis) {
		editingAxisId = axis.id;
		axisFormName = axis.name;
		axisFormSource = axis.source;
		axisFormDirLevel = axis.dirLevel ?? 1;
		axisFormFilterable = axis.filterable;
		axisFormOptionalInTitle = axis.optionalInTitle;
		axisForm.markPristine();
		axisDialogOpen = true;
		loadDirLevels();
	}

	function handleAxisSubmit(e: SubmitEvent) {
		e.preventDefault();
		saveAxis();
	}

	async function saveAxis() {
		if (axisBusy || dirLevelsPending) return;
		axisSaving = true;

		const fields = {
			name: axisFormName,
			source: axisFormSource,
			dirLevel: axisFormSource === 'dirLevel' ? (axisFormDirLevel ?? 1) : null,
			optionalInTitle: axisFormOptionalInTitle,
			filterable: axisFormFilterable
		} satisfies Omit<Axis, 'id' | 'position'>;
		// 保存後に値の辞書を取り直すか。応答で axes が置き換わる前に控えておく。
		const valuesCleared = axisValuesWillBeCleared;

		try {
			if (editingAxisId === null) {
				const {
					data: created,
					error,
					response
				} = await client.POST('/api/v1/contents/{id}/axes', {
					params: { path: { id: data.contentId } },
					body: axisRequestBody({ ...fields, position: axes.length })
				});
				if (!response.ok || !created) {
					errorDialog.show(errorMessage(error));
					return;
				}
				axes = [...axes, created];
				selectedAxisId = created.id;
			} else {
				const {
					data: updated,
					error,
					response
				} = await client.PUT('/api/v1/contents/{id}/axes/{axis_id}', {
					params: { path: { id: data.contentId, axis_id: editingAxisId } },
					body: axisRequestBody({ ...fields, position: editingAxis?.position ?? 0 })
				});
				if (!response.ok || !updated) {
					errorDialog.show(errorMessage(error));
					return;
				}
				axes = axes.map((axis) => (axis.id === updated.id ? updated : axis));
				// 抽出元・階層番号が変わった場合、選択中の値の辞書は古い抽出元のもとで
				// 取得した行のままなので、新しいsourceに対して取り直す(古い内容のまま
				// 保存すると意図せず全置換されてしまうため)。
				// 下の取り直しを待つ前に行を空にする。古い行のまま新しいsourceで予告が走らないように。
				if (selectedAxisId === updated.id && valuesCleared) {
					reloadAxisValues(updated.id);
				}
				// 軸名の変更はサーバー側でtitle_templateも書き換えるため、直後に取り直す。
				// 取り直しに失敗すると画面ごとエラーになるが、それはサーバーに届かないときで、
				// 画面を残しても保存できないため受け入れる。
				await invalidateAll();
			}
			axisDialogOpen = false;
		} catch {
			errorDialog.show(GENERIC_ERROR_MESSAGE());
		} finally {
			axisSaving = false;
		}
	}

	// 軸の並べ替え。並べ直した順に position を 0 から振り直し、値が変わった軸だけを PUT する。
	// マウス・タッチは行頭のつまみのドラッグ、キーボードは各行の上下ボタンで行う。
	let axisOrderSaving = $state(false);
	let axisList = $state<HTMLElement | null>(null);

	async function saveAxisOrder(ordered: Axis[]) {
		if (axisBusy) return;
		const renumbered = ordered.map((axis, position) => ({ ...axis, position }));
		const changed = renumbered.filter((axis, index) => axis.position !== ordered[index].position);
		if (changed.length === 0) return;

		axisOrderSaving = true;
		const previous = axes;
		// 応答を待たずに並べ替えて見せる。待つと、落とした行が一度元の位置に戻って見えるため。
		axes = renumbered;
		try {
			// 1件が例外で落ちても、残りの PUT が終わるまで待つ。途中で取り直すと、
			// 後から届いた PUT で DB だけが変わり、画面とずれるため。
			const settled = await Promise.allSettled(
				changed.map((axis) =>
					client.PUT('/api/v1/contents/{id}/axes/{axis_id}', {
						params: { path: { id: data.contentId, axis_id: axis.id } },
						body: axisRequestBody(axis)
					})
				)
			);
			if (settled.some((outcome) => outcome.status === 'rejected')) {
				errorDialog.show(GENERIC_ERROR_MESSAGE());
				await reloadAxesAfterFailedOrder(previous);
				return;
			}
			const results = settled.flatMap((outcome) =>
				outcome.status === 'fulfilled' ? [outcome.value] : []
			);
			const failed = results.find((result) => !result.response.ok || !result.data);
			if (failed) {
				errorDialog.show(errorMessage(failed.error));
				// 一部だけ成功した状態を残さないよう、サーバーの実態を取り直す。
				await reloadAxesAfterFailedOrder(previous);
				return;
			}
			const updated = new Map(
				results.flatMap((result) => (result.data ? [[result.data.id, result.data] as const] : []))
			);
			axes = axes.map((axis) => updated.get(axis.id) ?? axis);
		} catch {
			errorDialog.show(GENERIC_ERROR_MESSAGE());
			await reloadAxesAfterFailedOrder(previous);
		} finally {
			axisOrderSaving = false;
		}
	}

	/**
	 * 並べ替えの保存に失敗した後、軸を取り直す。取り直せなければ `fallback` に戻す。
	 */
	async function reloadAxesAfterFailedOrder(fallback: Axis[]) {
		try {
			const { data: refreshed, response } = await client.GET('/api/v1/contents/{id}/axes', {
				params: { path: { id: data.contentId } }
			});
			axes = response.ok && refreshed ? refreshed : fallback;
		} catch {
			axes = fallback;
		}
	}

	async function moveAxis(index: number, direction: -1 | 1) {
		const target = index + direction;
		if (target < 0 || target >= sortedAxes.length) return;
		const axisId = sortedAxes[index].id;
		await saveAxisOrder(moveItem(sortedAxes, index, target));
		// 保存中は上下ボタンが disabled になりフォーカスが外れるので、保存を終えてから戻す。
		// 失敗して元の並びに戻ることがあるので、位置は動かした軸から引き直す。
		await tick();
		focusMoveButton(
			axisList,
			sortedAxes.findIndex((axis) => axis.id === axisId),
			direction
		);
	}

	const axisZone = new SortableZone<Axis>({
		items: () => sortedAxes,
		key: (axis) => axis.id,
		locked: () => axisBusy || !editable,
		onReorder: (ordered) => {
			saveAxisOrder(ordered);
		}
	});

	// 削除確認ダイアログ。
	let deletingAxis = $state<Axis | null>(null);
	let axisDeleting = $state(false);

	// 軸そのものの操作(追加・編集・削除・並び替え)のいずれかが進行中か。
	// 値の辞書の保存と同時に走ると、互いの古い応答が新しい状態を上書きしうるため、
	// 両者を相互に無効化する。
	let axisStructureBusy = $derived(axisSaving || axisDeleting || axisOrderSaving);

	async function handleDeleteAxis() {
		if (axisBusy || deletingAxis === null) return;
		axisDeleting = true;
		const target = deletingAxis;

		try {
			const { error, response } = await client.DELETE('/api/v1/contents/{id}/axes/{axis_id}', {
				params: { path: { id: data.contentId, axis_id: target.id } }
			});
			if (!response.ok) {
				errorDialog.show(errorMessage(error));
				return;
			}
			axes = axes.filter((axis) => axis.id !== target.id);
			if (selectedAxisId === target.id) selectedAxisId = null;
			deletingAxis = null;
		} catch {
			errorDialog.show(GENERIC_ERROR_MESSAGE());
		} finally {
			axisDeleting = false;
		}
	}

	// 値の辞書。選択中の軸が変わるたびに取り直し、編集用にローカルへコピーする。
	type AxisValueRow = { rawValue: string; displayName: string; matchPosition: AxisMatchPosition };

	let axisValueRows = $state<AxisValueRow[]>([]);

	function toAxisValueRows(values: components['schemas']['AxisValueResponse'][]): AxisValueRow[] {
		return values.map((value) => ({
			rawValue: value.rawValue,
			displayName: value.displayName ?? '',
			matchPosition: value.matchPosition
		}));
	}
	// 保存中の軸。保存中に別の軸へ切り替えても、スピナーは保存している軸のボタンにだけ出す。
	let axisValuesSavingId = $state<number | null>(null);
	let axisValuesSaving = $derived(axisValuesSavingId !== null);
	// 読み込めるまで (読み込み中・失敗) は行の追加と保存を止める。読み込み中に足した行は届いた応答で消え、
	// 空の表を保存すると辞書が消えるため。
	let axisValuesLoaded = $state(false);
	// 取得に失敗したときの文言。表の場所に出す (ほかの画面の読み込み失敗と同じく、ダイアログにしない)。
	// 語の候補を「読み込んでいます...」のまま止めないためにも使う。
	let axisValuesLoadError = $state('');
	const axisValuesRequests = new LatestRequest();

	$effect(() => {
		const axisId = selectedAxisId;
		// 依存は選択中の軸だけにする。軸の編集後の invalidateAll で data が変わるたびに読み直すと、
		// 保存していない行の編集が消える。
		untrack(() => reloadAxisValues(axisId));
	});

	/**
	 * 値の辞書を取り直す (`null` なら空にするだけ)。前の行を残したまま次の GET を待つと、
	 * その間に保存すると前の内容が選択中の軸へ書き込まれてしまうので、先に空にする。
	 */
	function reloadAxisValues(axisId: number | null) {
		axisValuesRequests.invalidate();
		axisValueRows = [];
		axisValuesLoaded = false;
		axisValuesLoadError = '';
		if (axisId !== null) loadAxisValues(axisId);
	}

	async function loadAxisValues(axisId: number) {
		const isCurrent = axisValuesRequests.begin();
		try {
			const {
				data: values,
				error,
				response
			} = await client.GET('/api/v1/contents/{id}/axes/{axis_id}/values', {
				params: { path: { id: data.contentId, axis_id: axisId } }
			});
			if (!isCurrent()) return;
			if (!response.ok || !values) {
				axisValueRows = [];
				axisValuesLoadError = errorMessage(error);
				return;
			}
			axisValueRows = toAxisValueRows(values);
			axisValuesLoaded = true;
		} catch {
			if (isCurrent()) {
				axisValueRows = [];
				axisValuesLoadError = GENERIC_ERROR_MESSAGE();
			}
		}
	}

	function addAxisValueRow() {
		axisValueRows = [
			...axisValueRows,
			{ rawValue: '', displayName: '', matchPosition: 'anywhere' }
		];
	}

	function removeAxisValueRow(index: number) {
		axisValueRows = axisValueRows.filter((_, i) => i !== index);
	}

	// 行の並べ替え。表の行の順がそのまま並び順 (照合語なら照合の順) になり、
	// 表全体の保存で送る (→ docs/archive.md「軸の値の辞書と導出」)。
	// マウス・タッチは行頭のつまみのドラッグ、キーボードは各行の上下ボタンで行う。
	let axisValuesBody = $state<HTMLTableSectionElement | null>(null);

	let templateSaving = $state(false);

	// 軸まわりの保存 (軸そのもの・並べ替え・値の辞書・表示タイトル) のどれかが進行中か。
	// 進行中は、軸の追加・編集・削除と、軸の一覧・値の辞書の並べ替え、表示タイトルの保存を止める。
	// 軸の PUT も表示タイトルの PUT もコンテンツの全項目を送るので、重なると
	// 後から着いた方が、相手の更新前の値を書き戻してしまう。
	// 値の辞書は、保存の応答で表が作り直され、動かした結果が黙って消えるため。
	let axisBusy = $derived(axisStructureBusy || axisValuesSaving || templateSaving);

	/** 上下ボタンの読み上げに使う行の名前。追加したばかりで元の値が空の行は行番号にする。 */
	function axisValueRowName(row: AxisValueRow, index: number): string {
		return row.rawValue || m.archive_axes_values_row_number({ number: String(index + 1) });
	}

	async function moveAxisValueRow(index: number, direction: -1 | 1) {
		const target = index + direction;
		if (axisBusy || target < 0 || target >= axisValueRows.length) return;
		axisValueRows = moveItem(axisValueRows, index, target);
		await tick();
		focusMoveButton(axisValuesBody, target, direction);
	}

	const valueZone = new SortableZone<AxisValueRow>({
		items: () => axisValueRows,
		locked: () => axisBusy || !editable,
		onReorder: (ordered) => {
			axisValueRows = ordered;
		}
	});

	/**
	 * 上下ボタンで行を動かした後のフォーカス。端に着くと押したボタンが disabled になり
	 * フォーカスが外れるので、動かした行の同じ向きのボタン、押せなければ逆向きのボタンへ戻す。
	 */
	function focusMoveButton(rows: HTMLElement | null, index: number, direction: -1 | 1) {
		const row = rows?.children[index];
		const button =
			row?.querySelector<HTMLButtonElement>(
				`[data-move="${direction === -1 ? 'up' : 'down'}"]:not(:disabled)`
			) ?? row?.querySelector<HTMLButtonElement>('[data-move]:not(:disabled)');
		button?.focus();
	}

	function handleAxisValuesSubmit(e: SubmitEvent) {
		e.preventDefault();
		saveAxisValues();
	}

	/** 値の辞書の保存と、保存前の結果の問い合わせに送る body。 */
	function axisValuesRequestBody() {
		return {
			values: axisValueRows.map((row) => ({
				rawValue: row.rawValue,
				displayName: row.displayName.trim() === '' ? null : row.displayName,
				matchPosition: row.matchPosition
			}))
		};
	}

	async function saveAxisValues() {
		const axis = selectedAxis;
		if (axis === null || axisBusy || !axisValuesLoaded) return;
		const axisId = axis.id;
		// サーバーの英語の 422 を見せる代わりに、予告の欄と同じ文言で止める。
		const rawValues = axisValueRows.map((row) => row.rawValue);
		if (axisValuesPreviewBlocker(rawValues, axis.source) === 'tooMany') {
			errorDialog.show(
				m.archive_axes_preview_too_many({ limit: formatNumber(FILENAME_WORD_VALUES_LIMIT) })
			);
			return;
		}
		axisValuesSavingId = axisId;

		try {
			const {
				data: values,
				error,
				response
			} = await client.PUT('/api/v1/contents/{id}/axes/{axis_id}/values', {
				params: { path: { id: data.contentId, axis_id: axisId } },
				body: axisValuesRequestBody()
			});
			// 成否は、保存中に別の軸へ切り替えられていても知らせる (成功のトーストには軸名が入る)。
			if (!response.ok || !values) {
				errorDialog.show(errorMessage(error));
				return;
			}
			toast.success(m.archive_axes_values_saved_toast({ name: axis.name }));
			// 保存中に別の軸へ切り替えられていたら、その軸の表に古い応答を書き込まない。
			if (selectedAxisId !== axisId) return;
			axisValueRows = toAxisValueRows(values);
		} catch {
			errorDialog.show(GENERIC_ERROR_MESSAGE());
		} finally {
			axisValuesSavingId = null;
		}
	}

	// 照合語を決める手がかり (→ docs/archive.md「軸の設定を助ける表示」)。
	// 依存を軸オブジェクトではなく値にする。軸を編集すると axes が新しい参照に置き換わるため。
	let selectedAxisSource = $derived(selectedAxis?.source ?? null);

	// ファイル名の語の候補。再スキャンで変わるので、軸を選び直すたびに取り直す。
	type FilenameWordsState =
		| { status: 'idle' }
		| { status: 'loading' }
		| { status: 'loaded'; response: FilenameWordsResponse }
		| { status: 'failed' };

	let filenameWords = $state<FilenameWordsState>({ status: 'idle' });
	// 検索語。空なら既定で隠す語を出さず、入れると隠す語も含めて探す (→ docs/archive.md「軸の設定を助ける表示」)。
	let wordQuery = $state('');
	let shownWordCount = $state(WORD_CANDIDATES_PAGE_SIZE);
	let wordChipList = $state<HTMLElement | null>(null);
	const filenameWordsRequests = new LatestRequest();

	$effect(() => {
		const wanted = selectedAxisId !== null && selectedAxisSource === 'filenameWord';
		untrack(() => reloadFilenameWords(wanted));
	});

	function reloadFilenameWords(wanted: boolean) {
		filenameWordsRequests.invalidate();
		filenameWords = { status: 'idle' };
		wordQuery = '';
		shownWordCount = WORD_CANDIDATES_PAGE_SIZE;
		if (wanted) loadFilenameWords();
	}

	async function loadFilenameWords() {
		const isCurrent = filenameWordsRequests.begin();
		filenameWords = { status: 'loading' };
		try {
			const { data: words, response } = await client.GET('/api/v1/contents/{id}/filename-words', {
				params: { path: { id: data.contentId } }
			});
			if (!isCurrent()) return;
			filenameWords =
				response.ok && words ? { status: 'loaded', response: words } : { status: 'failed' };
		} catch {
			if (isCurrent()) filenameWords = { status: 'failed' };
		}
	}

	let selectedRawValues = $derived(axisValueRows.map((row) => row.rawValue));
	let wordCandidateList = $derived(
		filenameWords.status === 'loaded'
			? wordCandidates(filenameWords.response.words, selectedRawValues, wordQuery)
			: []
	);
	let shownWordCandidates = $derived(wordCandidateList.slice(0, shownWordCount));

	function handleWordQueryInput() {
		// 検索語を変えたら、また最初の数から出す。
		shownWordCount = WORD_CANDIDATES_PAGE_SIZE;
	}

	// 押したボタンは消えることがあるので、フォーカスを body に落とさないよう移す。
	async function showMoreWords() {
		const firstNew = shownWordCount;
		shownWordCount += WORD_CANDIDATES_PAGE_SIZE;
		await tick();
		wordChipList?.querySelectorAll<HTMLButtonElement>('button')[firstNew]?.focus();
	}

	async function showFewerWords() {
		shownWordCount = WORD_CANDIDATES_PAGE_SIZE;
		await tick();
		document.querySelector<HTMLButtonElement>('[data-words-more]')?.focus();
	}

	async function addWordRow(word: string, index: number) {
		axisValueRows = [
			...axisValueRows,
			{ rawValue: word, displayName: '', matchPosition: 'anywhere' }
		];
		// 押した語は候補から消えてフォーカスが外れるので、その位置に詰めてきた語 (無ければ1つ前) へ移す。
		await tick();
		const chips = wordChipList?.querySelectorAll<HTMLButtonElement>('button');
		if (chips && chips.length > 0) chips[Math.min(index, chips.length - 1)].focus();
		// 最後の候補を押して候補が無くなったら、同じ区画の検索欄へ移す (body に落とさない)。
		else document.getElementById('axis-words-query')?.focus();
	}

	// 保存前の表での結果。
	/** 表を変えてから問い合わせるまでの待ち。語を打っている間、1文字ごとに問い合わせないため。 */
	const PREVIEW_DELAY_MS = 300;

	// 問い合わせ中も前の結果を出したままにする。表を変えるたびに消すと、件数の行がちらつくため。
	let preview = $state<AxisValuesPreview | null>(null);
	let previewStatus = $state<'idle' | 'incomplete' | 'tooMany' | 'loading' | 'loaded' | 'failed'>(
		'idle'
	);
	const previewRequests = new LatestRequest();
	// 行ごとの結果を元の値 (サーバーと同じく前後の空白を除いたもの) で引く。問い合わせ中に行を
	// 足し引きしても、位置で引いて別の行の結果を出さないため。
	let previewRows = $state(new Map<string, AxisValuesPreviewRow>());

	$effect(() => {
		const axisId = selectedAxisId;
		const rawValues = axisValueRows.map((row) => row.rawValue);
		// 照合する位置を変えたときも数え直す。
		for (const row of axisValueRows) void row.matchPosition;
		const source = selectedAxisSource;
		if (axisId === null || source === null || !axisValuesLoaded) {
			untrack(() => resetPreview('idle'));
			return;
		}
		// 空・重複の行や、語の軸での上限超えがあると 422 になる。前の結果も、今の表とは合わないので出さない。
		const blocker = axisValuesPreviewBlocker(rawValues, source);
		if (blocker !== null) {
			untrack(() => resetPreview(blocker));
			return;
		}
		// 直前の「入力中の行があります」「数えられませんでした」は今の表に合わないので、待つ間は数えている表示にする。
		untrack(() => {
			if (
				previewStatus === 'incomplete' ||
				previewStatus === 'tooMany' ||
				previewStatus === 'failed'
			)
				previewStatus = 'loading';
		});
		const timer = setTimeout(() => loadPreview(axisId), PREVIEW_DELAY_MS);
		return () => {
			clearTimeout(timer);
			previewRequests.invalidate();
		};
	});

	function resetPreview(status: 'idle' | 'incomplete' | 'tooMany') {
		previewRequests.invalidate();
		preview = null;
		previewRows = new Map();
		previewStatus = status;
	}

	async function loadPreview(axisId: number) {
		const isCurrent = previewRequests.begin();
		previewStatus = 'loading';
		const body = axisValuesRequestBody();
		try {
			const { data: result, response } = await client.POST(
				'/api/v1/contents/{id}/axes/{axis_id}/values/preview',
				{
					params: { path: { id: data.contentId, axis_id: axisId } },
					body
				}
			);
			if (!isCurrent()) return;
			preview = response.ok && result ? result : null;
			previewRows = new Map(
				(preview?.rows ?? []).map((row, index) => [body.values[index].rawValue.trim(), row])
			);
			previewStatus = preview === null ? 'failed' : 'loaded';
		} catch {
			if (isCurrent()) {
				preview = null;
				previewRows = new Map();
				previewStatus = 'failed';
			}
		}
	}

	// アイテムが無ければ、候補も結果も出しようがないので、再スキャンへの案内に替える。
	let archiveHasNoItems = $derived(
		(filenameWords.status === 'loaded' && filenameWords.response.itemCount === 0) ||
			preview?.itemCount === 0
	);

	// 表示タイトルのテンプレート。書き込み可能な$derivedにし、bind:valueでの編集はローカルに
	// 保持しつつ、currentContentが更新された(軸のリネームやテンプレート保存の成功)
	// タイミングでは自動的にサーバー値へ再同期される。
	let titleTemplateInput = $derived(currentContent.titleTemplate ?? '');
	// dirty判定は他のフォームと同じくFormDirtyStateに揃える(→ AGENTS.md「コードの規約」)。
	// currentContentの更新 (再同期) のたびに基準値を引き直す。
	const templateForm = new FormDirtyState(() => ({ titleTemplate: titleTemplateInput }));
	$effect(() => {
		void currentContent;
		// markPristine() 内で titleTemplateInput を読むため、untrack しないと
		// 入力するたびに effect が再実行され、常に pristine 扱いになってしまう。
		untrack(() => templateForm.markPristine());
	});

	// カーソル位置に部品を挿入するため、入力欄の DOM 参照を持つ。
	let titleTemplateInputEl = $state<HTMLInputElement | null>(null);

	// テンプレート入力欄のカーソル位置 (選択範囲があればその範囲) に {軸名}・{fileName} を挿入する。
	// 押しても保存はしない。
	function insertTemplateToken(name: string) {
		const token = `{${name}}`;
		const el = titleTemplateInputEl;
		const current = titleTemplateInput;
		const start = el?.selectionStart ?? current.length;
		const end = el?.selectionEnd ?? current.length;
		titleTemplateInput = current.slice(0, start) + token + current.slice(end);
		// フォーカスは同期的に戻す。iOS/iPadOS はユーザー操作の最中でないとソフトキーボードを
		// 開き直さず、await をはさむと入力欄にキーボードが戻らないため。
		el?.focus();
		// カーソルは、挿入後の値が DOM に反映されてから、挿入した文字列の直後へ置く。
		void tick().then(() => {
			if (!el) return;
			const caret = start + token.length;
			el.setSelectionRange(caret, caret);
		});
	}

	function handleTemplateSubmit(e: SubmitEvent) {
		e.preventDefault();
		saveTemplate();
	}

	async function saveTemplate() {
		if (axisBusy) return;
		templateSaving = true;

		try {
			const titleTemplate = titleTemplateInput.trim() === '' ? null : titleTemplateInput;
			const {
				data: updated,
				error,
				response
			} = await client.PUT('/api/v1/contents/{id}', {
				params: { path: { id: data.contentId } },
				body: {
					description: currentContent.description ?? null,
					extensions: currentContent.extensions ?? null,
					parentId: currentContent.parentId ?? null,
					path: currentContent.path ?? null,
					title: currentContent.title,
					titleTemplate,
					url: currentContent.url ?? null,
					visibility: currentContent.visibility
				}
			});
			if (!response.ok || !updated) {
				errorDialog.show(errorMessage(error));
				return;
			}
			currentContent = updated;
			toast.success(m.archive_axes_template_saved_toast());
		} catch {
			errorDialog.show(GENERIC_ERROR_MESSAGE());
		} finally {
			templateSaving = false;
		}
	}
</script>

<!-- ドラッグで並べ替えるつまみ (軸の一覧と値の辞書の表で共通)。キーボードでは各行の上下ボタンを使うので、
     フォーカスを受けず読み上げもしない。 -->
{#snippet dragHandle(handle: Attachment<HTMLElement>, extraClass?: string)}
	<span
		{@attach handle}
		aria-hidden="true"
		class={[dragHandleClass, extraClass, axisBusy ? 'opacity-50' : 'cursor-grab']}
		oncontextmenu={(event) => event.preventDefault()}
	>
		<GripVerticalIcon class="size-4" />
	</span>
{/snippet}

<!-- アイテムが無いときの案内と、再スキャンできる「アイテム」タブへのリンク (軸のダイアログと値の辞書で共通)。 -->
{#snippet noItemsNotice(message: string)}
	<div class="flex flex-col">
		<p class="text-sm text-muted-foreground">{message}</p>
		<a
			href={resolve('/admin/contents/[id]/items', { id: String(data.contentId) })}
			class="inline-flex h-11 w-fit items-center text-sm text-primary underline underline-offset-4"
		>
			{m.archive_axes_items_link()}
		</a>
	</div>
{/snippet}

{#snippet matchPositionSelect(row: AxisValueRow, index: number)}
	<!-- 同じ行の入力欄と高さを揃えて 44px にする (→ docs/ui.md「UI 全般」)。 -->
	<Select.Root
		type="single"
		bind:value={row.matchPosition}
		disabled={axisStructureBusy || !editable}
	>
		<Select.Trigger
			class="w-full"
			aria-label={m.archive_axes_values_match_position_label({
				value: axisValueRowName(row, index),
				position: matchPositionLabel(row.matchPosition)
			})}
		>
			{matchPositionLabel(row.matchPosition)}
		</Select.Trigger>
		<Select.Content>
			{#each MATCH_POSITIONS as position (position)}
				<Select.Item value={position}>{matchPositionLabel(position)}</Select.Item>
			{/each}
		</Select.Content>
	</Select.Root>
{/snippet}

<!-- 語が何を指すかを確かめるため、その行で値が決まるファイルを数件開けるようにする (→ docs/archive.md「軸の設定を助ける表示」)。
     件数は語の候補と同じく、語の右に淡い文字で「N 件」と出す。 -->
{#snippet rowFiles(row: AxisValueRow, index: number)}
	{@const result = previewRows.get(row.rawValue.trim())}
	<DropdownMenu.Root>
		<DropdownMenu.Trigger>
			{#snippet child({ props })}
				<Button
					{...props}
					variant="ghost"
					size="sm"
					class="min-w-11 shrink-0 text-muted-foreground tabular-nums"
					disabled={result === undefined || result.matched === 0}
					aria-label={m.archive_axes_values_row_files_label({
						value: axisValueRowName(row, index),
						count: formatNumber(result?.matched ?? 0)
					})}
				>
					<!-- 見出しの無い数なので、単位を付けて何の数かを示す。 -->
					{result === undefined
						? '–'
						: m.archive_axes_values_row_files_count({ count: formatNumber(result.matched) })}
				</Button>
			{/snippet}
		</DropdownMenu.Trigger>
		{#if result !== undefined}
			<DropdownMenu.Content align="start" class="max-w-80 sm:max-w-lg">
				<DropdownMenu.Label>{m.archive_axes_values_row_files_heading()}</DropdownMenu.Label>
				{#each result.examples as item (item.id)}
					<DropdownMenu.Item class="wrap-anywhere">
						{#snippet child({ props })}
							<a
								{...props}
								href={archiveItemManageDownloadHref(data.contentId, item.id)}
								target="_blank"
								rel="external noopener noreferrer">{item.relPath}</a
							>
						{/snippet}
					</DropdownMenu.Item>
				{/each}
				{#if result.matched > result.examples.length}
					<p class="px-2 py-1.5 text-sm text-muted-foreground">
						{m.archive_axes_values_row_files_more({
							count: formatNumber(result.matched - result.examples.length)
						})}
					</p>
				{/if}
			</DropdownMenu.Content>
		{/if}
	</DropdownMenu.Root>
{/snippet}

<!-- 保存前の表での結果。問い合わせ中は前の結果を出したままにする。 -->
{#snippet previewSummary()}
	<div class="flex flex-col">
		<p class="text-sm" aria-live="polite">
			{#if previewStatus === 'incomplete'}
				<span class="text-muted-foreground">{m.archive_axes_preview_incomplete()}</span>
			{:else if previewStatus === 'tooMany'}
				<span class="text-muted-foreground"
					>{m.archive_axes_preview_too_many({
						limit: formatNumber(FILENAME_WORD_VALUES_LIMIT)
					})}</span
				>
			{:else if previewStatus === 'failed'}
				<span class="text-muted-foreground">{m.archive_axes_preview_failed()}</span>
			{:else if preview !== null}
				{m.archive_axes_preview_summary({
					matched: formatNumber(preview.matched),
					unset: formatNumber(preview.unset)
				})}
			{:else if previewStatus === 'loading'}
				<Spinner class="inline-block align-middle" aria-label={m.archive_axes_preview_loading()} />
			{/if}
		</p>
		{#if preview !== null && preview.unsetExamples.length > 0 && previewStatus !== 'failed'}
			<!-- 開閉ボタンの高さを 44px にする (text-sm の行 20px + 上下 12px)。 -->
			<details>
				<summary class="w-fit cursor-pointer py-3 text-sm text-muted-foreground">
					{m.archive_axes_preview_unset_examples()}
				</summary>
				<ul class="flex flex-col gap-1 text-sm text-muted-foreground">
					{#each preview.unsetExamples as relPath (relPath)}
						<li class="wrap-anywhere">{relPath}</li>
					{/each}
				</ul>
			</details>
		{/if}
	</div>
{/snippet}

<!-- 見出し (タイトル・パス) とタブは [id]/+layout.svelte が出す。 -->
<div class={[adminPageClass, 'flex flex-col gap-4']}>
	<!-- 軸の一覧と値の辞書を横に並べるのは lg から。md で並べると辞書の表の幅が足りず、
			     横にはみ出す (→ docs/ui.md「UI 全般」の「管理画面の表は横スクロールさせない」)。 -->
	<!-- 区画の間は 48px 空け、その中ほどに罫線を引く (→ docs/ui.md「UI 全般」の「余白と区切り」)。 -->
	<div class="flex flex-col lg:flex-row">
		<!-- 左カラムの幅は、つまみ・上下ボタン・編集・削除を並べても抽出元
				     (「ファイル名に含まれる語」) が1行に収まるように取る。 -->
		<div
			class="flex flex-col gap-2 border-b pb-6 lg:w-108 lg:shrink-0 lg:border-r lg:border-b-0 lg:pr-6 lg:pb-0"
		>
			<div class="flex items-center justify-between">
				<h2 class="text-base font-semibold">{m.archive_axes_section_title()}</h2>
				{#if editable}
					<div class="flex flex-wrap justify-end gap-2">
						<Button variant="outline" disabled={axisBusy} onclick={() => (aiDialogOpen = true)}>
							{m.archive_axes_ai_button()}
						</Button>
						<Button variant="outline" disabled={axisBusy} onclick={openCreateAxisDialog}>
							{m.archive_axes_add_button()}
						</Button>
					</div>
				{/if}
			</div>
			<a
				href={resolve('/help/[slug]', { slug: 'archive-setup' })}
				target="_blank"
				rel="noopener"
				class="inline-flex h-11 w-fit items-center text-xs text-muted-foreground underline underline-offset-4"
			>
				{m.archive_axes_help_link()}
			</a>

			{#if sortedAxes.length === 0}
				<p class="text-sm text-muted-foreground">{m.archive_axes_empty()}</p>
			{:else}
				<div bind:this={axisList} {@attach axisZone.zone} class="flex flex-col gap-2">
					{#each axisZone.entries as entry, index (entry.id)}
						{@const axis = entry.item}
						<!-- 上下のアイコンボタンは押せる範囲を四方に 6px 広げているので、隣と重ならないよう
								     間隔を 12px 取る (→ docs/ui.md「UI 全般」)。
								     ドラッグで運ぶ複製にも地の色が要るので、選択していない行にも背景を付ける。 -->
						<div
							animate:flip={{ duration: SORTABLE_FLIP_DURATION_MS }}
							class={[
								'relative flex items-center gap-3 rounded-lg border p-2',
								selectedAxisId === axis.id ? 'bg-accent' : 'bg-background',
								axisZone.isPressed(entry) && 'opacity-60 ring-2 ring-primary'
							]}
						>
							{#if editable}
								{@render dragHandle(axisZone.handle(entry.id), 'shrink-0')}
							{/if}
							<button
								type="button"
								class="relative min-w-0 flex-1 text-left after:absolute after:inset-x-0 after:-inset-y-1"
								onclick={() => (selectedAxisId = axis.id)}
							>
								<p class="truncate text-sm font-medium">{axis.name}</p>
								<!-- 「フォルダーの第1階層」が「第1階 / 層」と割れないようにする。 -->
								<p class="text-xs break-keep text-muted-foreground">{axisSourceLabel(axis)}</p>
								{#if !axis.filterable}
									<p class="text-xs text-muted-foreground">{m.archive_axes_not_filterable()}</p>
								{/if}
							</button>
							{#if editable}
								<!-- 上下ボタンは lg 以上だけ。狭い幅はつまみに譲る (→ docs/ui.md「UI 全般」)。 -->
								<Button
									variant="ghost"
									size="icon"
									class="hidden lg:inline-flex"
									data-move="up"
									disabled={index === 0 || axisBusy}
									aria-label={m.archive_axes_move_up({ name: axis.name })}
									onclick={() => moveAxis(index, -1)}
								>
									<ChevronUpIcon class="size-4" />
								</Button>
								<Button
									variant="ghost"
									size="icon"
									class="hidden lg:inline-flex"
									data-move="down"
									disabled={index === sortedAxes.length - 1 || axisBusy}
									aria-label={m.archive_axes_move_down({ name: axis.name })}
									onclick={() => moveAxis(index, 1)}
								>
									<ChevronDownIcon class="size-4" />
								</Button>
								<Button
									variant="outline"
									size="sm"
									disabled={axisBusy}
									aria-label={m.archive_axes_edit_label({ name: axis.name })}
									onclick={() => openEditAxisDialog(axis)}
								>
									{m.action_edit()}
								</Button>
								<Button
									variant="destructive"
									size="sm"
									disabled={axisBusy}
									aria-label={m.archive_axes_delete_label({ name: axis.name })}
									onclick={() => (deletingAxis = axis)}
								>
									{m.action_delete()}
								</Button>
							{/if}
						</div>
					{/each}
				</div>
			{/if}
		</div>

		<div class="flex min-w-0 flex-1 flex-col pt-6 lg:pt-0 lg:pl-6">
			{#if selectedAxis !== null}
				{@const axis = selectedAxis}
				<div class="flex flex-col gap-6 border-b pb-6">
					<div class="flex flex-col gap-2">
						<h2 id="axis-values-heading" class="text-base font-semibold">
							{m.archive_axes_values_heading({ name: axis.name })}
						</h2>
						<p class="text-sm text-muted-foreground">
							{axis.source === 'dirLevel'
								? m.archive_axes_values_hint_dir_level()
								: m.archive_axes_values_hint_filename_word()}
						</p>
					</div>

					{#if archiveHasNoItems}
						{@render noItemsNotice(m.archive_axes_values_no_items())}
					{:else if axis.source === 'filenameWord' && editable}
						<!-- 表を埋める手がかりなので、枠で囲って表と見分ける (→ docs/ui.md「UI 全般」の「余白と区切り」)。 -->
						<section
							class="flex flex-col gap-6 rounded-lg border bg-background p-4"
							aria-labelledby="axis-words-title"
						>
							<div class="flex flex-col gap-2">
								<h3 id="axis-words-title" tabindex="-1" class="text-sm font-semibold">
									{m.archive_axes_words_title()}
								</h3>
								<p id="axis-words-hint" class="text-sm text-muted-foreground">
									{m.archive_axes_words_hint()}
								</p>
							</div>
							<div class="flex flex-col gap-2">
								<Field.Field>
									<Field.FieldLabel for="axis-words-query">
										{m.archive_axes_words_query_label()}
									</Field.FieldLabel>
									<Input
										id="axis-words-query"
										type="search"
										bind:value={wordQuery}
										oninput={handleWordQueryInput}
										aria-describedby="axis-words-query-help"
									/>
									<Field.FieldDescription id="axis-words-query-help">
										{m.archive_axes_words_query_help()}
									</Field.FieldDescription>
								</Field.Field>
								{#if filenameWords.status === 'failed' || axisValuesLoadError !== ''}
									<p class="text-sm text-muted-foreground">{m.archive_axes_words_fetch_failed()}</p>
								{:else if filenameWords.status !== 'loaded' || !axisValuesLoaded}
									<Spinner aria-label={m.archive_axes_words_loading()} />
								{:else if wordCandidateList.length === 0}
									<p class="text-sm text-muted-foreground">
										{wordQuery.trim() === ''
											? m.archive_axes_words_empty()
											: m.archive_axes_words_no_match()}
									</p>
								{:else}
									<!-- 語は押せる範囲を上下に 8px 広げているので、折り返した行と重ならないよう行間を 16px 取る
								     (→ docs/ui.md「UI 全般」)。 -->
									<ul bind:this={wordChipList} class="flex flex-wrap gap-x-2 gap-y-4 py-2">
										{#each shownWordCandidates as word, index (word.word)}
											<li>
												<Button
													type="button"
													variant="outline"
													size="sm"
													aria-describedby="axis-words-hint"
													disabled={axisStructureBusy}
													onclick={() => addWordRow(word.word, index)}
												>
													{word.word}
													<span class="text-muted-foreground">{formatNumber(word.count)}</span>
												</Button>
											</li>
										{/each}
									</ul>
									<div class="flex flex-wrap gap-x-4">
										{#if wordCandidateList.length > shownWordCount}
											<Button
												type="button"
												variant="link"
												class="w-fit px-0"
												data-words-more
												onclick={showMoreWords}
											>
												{m.archive_axes_words_more()}
											</Button>
										{/if}
										{#if shownWordCount > WORD_CANDIDATES_PAGE_SIZE && wordCandidateList.length > WORD_CANDIDATES_PAGE_SIZE}
											<Button
												type="button"
												variant="link"
												class="w-fit px-0"
												onclick={showFewerWords}
											>
												{m.archive_axes_words_less()}
											</Button>
										{/if}
									</div>
								{/if}
							</div>
						</section>
					{/if}

					<!-- 見出しで名前を付ける。保存ボタンが2つある画面なので、どちらのフォームかを
					     読み上げにも e2e にも区別させる。 -->
					<form
						onsubmit={handleAxisValuesSubmit}
						aria-labelledby="axis-values-heading"
						class="flex flex-col gap-6"
					>
						<!-- 件数は表の結果なので、表のすぐ下に付ける。 -->
						<div class="flex flex-col gap-2">
							{#if axisValuesLoadError !== ''}
								<p class="text-sm text-destructive">{axisValuesLoadError}</p>
							{:else if axisValueRows.length === 0}
								<p class="text-sm text-muted-foreground">{m.archive_axes_values_empty()}</p>
							{:else}
								<Table.Root>
									<Table.Header>
										<Table.Row>
											<Table.Head class="w-10"></Table.Head>
											<!-- 元の値は短い語なので幅を固定し、余りを表示名に回す。狭い幅では中身に任せる。
											     語の軸では、行ごとの件数のボタンを並べる分だけ広げる。 -->
											<Table.Head class={axis.source === 'filenameWord' ? 'sm:w-44' : 'sm:w-32'}
												>{m.archive_axes_values_column_raw()}</Table.Head
											>
											{#if axis.source === 'filenameWord'}
												<!-- 選んだ位置で幅が変わらないよう、いちばん長い選択肢に合わせて固定する。
											     狭い幅では列ごと隠し、元の値の欄の下に回す。 -->
												<Table.Head class={['sm:w-44', columnFromSmClass]}
													>{m.archive_axes_values_column_match_position()}</Table.Head
												>
											{/if}
											<Table.Head>{m.archive_axes_values_column_display()}</Table.Head>
											<!-- 上下ボタンの列は lg 以上だけ。狭い幅はつまみに譲る (→ docs/ui.md「UI 全般」)。 -->
											<Table.Head class={['w-24', columnFromLgClass]}></Table.Head>
											{#if axis.source === 'filenameWord'}
												<Table.Head class="w-10"></Table.Head>
											{/if}
										</Table.Row>
									</Table.Header>
									<Table.Body bind:ref={axisValuesBody} {@attach valueZone.zone}>
										<!-- 入力欄のセルは上下に余白を取る。行は 44px を超える (→ docs/ui.md「UI 全般」)。 -->
										{#each valueZone.entries as entry, index (entry.id)}
											{@const row = entry.item}
											<!-- animate: は部品には付けられないので tr を直接書く。class は table-row.svelte と揃える。
												     ドラッグで運ぶ複製にも地の色が要るので、背景を付ける。 -->
											<tr
												animate:flip={{ duration: SORTABLE_FLIP_DURATION_MS }}
												data-slot="table-row"
												class={[
													'border-b border-divider bg-background transition-colors hover:bg-muted/50',
													valueZone.isPressed(entry) && 'opacity-60 [&>td]:bg-accent'
												]}
											>
												<Table.Cell class="py-0">
													{#if editable}
														{@render dragHandle(valueZone.handle(entry.id))}
													{/if}
												</Table.Cell>
												<Table.Cell class="py-1.5">
													{#if axis.source === 'dirLevel'}
														<span class="text-sm">{row.rawValue}</span>
													{:else}
														<!-- スマートフォン幅では横に並べると入力欄が潰れるので、件数を下に回す。
														     件数のボタンは押せる範囲を上に 8px 広げているので、入力欄と重ならないよう 8px 空ける。 -->
														<div
															class="flex flex-col items-start gap-2 sm:flex-row sm:items-center sm:gap-1"
														>
															<Input
																bind:value={row.rawValue}
																aria-label={m.archive_axes_values_raw_label({
																	number: String(index + 1)
																})}
																required
																disabled={axisStructureBusy || !editable}
															/>
															<!-- スマートフォン幅では照合する位置の列を隠し、ここに回す。
															     4つの欄を横に並べると、元の値と表示名に字を打つ幅が残らないため。 -->
															<div class="w-full sm:hidden">
																{@render matchPositionSelect(row, index)}
															</div>
															{@render rowFiles(row, index)}
														</div>
													{/if}
												</Table.Cell>
												{#if axis.source === 'filenameWord'}
													<Table.Cell class={['py-1.5', columnFromSmClass]}>
														{@render matchPositionSelect(row, index)}
													</Table.Cell>
												{/if}
												<Table.Cell class="py-1.5">
													<Input
														bind:value={row.displayName}
														aria-label={m.archive_axes_values_display_label({
															value: axisValueRowName(row, index)
														})}
														disabled={axisStructureBusy || !editable}
													/>
												</Table.Cell>
												<Table.Cell class={['py-0', columnFromLgClass]}>
													<!-- 押せる範囲を四方に 6px 広げているので、隣と重ならないよう間隔を 12px 取る
														     (→ docs/ui.md「UI 全般」。軸の一覧の上下ボタンと同じ)。 -->
													{#if editable}
														<div class="flex justify-end gap-3">
															<Button
																type="button"
																variant="ghost"
																size="icon"
																data-move="up"
																disabled={index === 0 || axisBusy}
																aria-label={m.archive_axes_values_move_up({
																	value: axisValueRowName(row, index)
																})}
																onclick={() => moveAxisValueRow(index, -1)}
															>
																<ChevronUpIcon class="size-4" />
															</Button>
															<Button
																type="button"
																variant="ghost"
																size="icon"
																data-move="down"
																disabled={index === axisValueRows.length - 1 || axisBusy}
																aria-label={m.archive_axes_values_move_down({
																	value: axisValueRowName(row, index)
																})}
																onclick={() => moveAxisValueRow(index, 1)}
															>
																<ChevronDownIcon class="size-4" />
															</Button>
														</div>
													{/if}
												</Table.Cell>
												{#if axis.source === 'filenameWord'}
													<Table.Cell>
														{#if editable}
															<Button
																type="button"
																variant="ghost"
																size="icon-sm"
																disabled={axisStructureBusy}
																aria-label={m.archive_axes_values_remove_row({
																	value: axisValueRowName(row, index)
																})}
																onclick={() => removeAxisValueRow(index)}
															>
																<XIcon class="size-4" />
															</Button>
														{/if}
													</Table.Cell>
												{/if}
											</tr>
										{/each}
									</Table.Body>
								</Table.Root>
							{/if}

							{#if !archiveHasNoItems}
								{@render previewSummary()}
							{/if}
						</div>

						<!-- 値を追加するボタンは保存ボタンの右隣に置く。押しても保存はしないので、確定の保存とは
						     別の見た目 (枠線だけ) にする。type="button" で送信しない。 -->
						{#if editable}
							<div class="flex flex-wrap items-center gap-3">
								<LoadingButton
									type="submit"
									loading={axisValuesSavingId === selectedAxisId}
									disabled={axisStructureBusy || axisValuesSaving || !axisValuesLoaded}
								>
									{m.action_save()}
								</LoadingButton>
								{#if axis.source === 'filenameWord'}
									<Button
										type="button"
										variant="outline"
										disabled={axisStructureBusy || !axisValuesLoaded}
										onclick={addAxisValueRow}
									>
										{m.archive_axes_values_add_row()}
									</Button>
								{/if}
							</div>
						{/if}
					</form>
				</div>
			{/if}

			<form
				onsubmit={handleTemplateSubmit}
				aria-labelledby="title-template-heading"
				class={['flex flex-col gap-6', selectedAxis !== null && 'pt-6']}
			>
				{#snippet tokenButton(name: string)}
					<li>
						<Button
							type="button"
							variant="outline"
							size="sm"
							aria-describedby="title-template-tokens-label"
							onclick={() => insertTemplateToken(name)}
						>
							{`{${name}}`}
						</Button>
					</li>
				{/snippet}
				<!-- 見出しを別に見せるとラベルと同じ文言が2回並ぶので、ラベルを見出しの太さにして区切りを兼ねる。
				     見出しで移動する読み上げのために、見出し自体は見えない形で残す。 -->
				<h2 id="title-template-heading" class="sr-only">{m.archive_axes_template_title()}</h2>
				<Field.Field>
					<!-- 区画の見出しを兼ねるので、ほかの区画の見出しと同じ大きさにする。 -->
					<Field.FieldLabel for="title-template" class="text-base font-semibold">
						{m.archive_axes_template_title()}
					</Field.FieldLabel>
					<Input
						id="title-template"
						bind:ref={titleTemplateInputEl}
						bind:value={titleTemplateInput}
						disabled={!editable}
						aria-describedby="title-template-help"
					/>
					<!-- 入力欄についての注意なので、入力欄のすぐ下に置く。 -->
					<Field.FieldDescription id="title-template-help"
						>{m.archive_axes_template_help()}</Field.FieldDescription
					>
				</Field.Field>
				{#if editable}
					<div class="flex flex-col gap-2">
						<Field.FieldDescription id="title-template-tokens-label">
							{m.archive_axes_template_available_axes_label()}
						</Field.FieldDescription>
						<!-- 押すとカーソル位置にプレースホルダーを挿入する。保存はしないので、確定に見える差し色を
					     使わず枠線だけにする (→ docs/ui.md「UI 全般」。値の辞書の語の候補と同じ)。
					     押せる範囲を上下に広げるので、折り返した行と重ならないよう行間を空ける。 -->
						<ul class="flex flex-wrap gap-x-2 gap-y-4 py-2">
							{#each sortedAxes as axis (axis.id)}
								{@render tokenButton(axis.name)}
							{/each}
							<!-- 軸ではないが同じようにテンプレートで使える (→ docs/archive.md「表示タイトル」)。 -->
							{@render tokenButton(FILE_NAME_PLACEHOLDER)}
						</ul>
						<Field.FieldDescription
							>{m.archive_axes_template_file_name_help()}</Field.FieldDescription
						>
					</div>
				{/if}
				{#if editable}
					<div>
						<LoadingButton
							type="submit"
							loading={templateSaving}
							disabled={axisBusy || !templateForm.dirty}
						>
							{m.action_save()}
						</LoadingButton>
					</div>
				{/if}
			</form>
		</div>
	</div>
</div>

<Dialog.Root bind:open={axisDialogOpen}>
	<Dialog.Content
		interactOutsideBehavior={axisForm.dirty ? 'ignore' : 'close'}
		escapeKeydownBehavior={axisForm.dirty ? 'ignore' : 'close'}
	>
		<Dialog.Header>
			<Dialog.Title>
				{editingAxisId === null
					? m.archive_axes_dialog_create_title()
					: m.archive_axes_dialog_edit_title()}
			</Dialog.Title>
		</Dialog.Header>
		<form onsubmit={handleAxisSubmit} class="flex flex-col gap-6">
			<Field.FieldGroup>
				<Field.Field>
					<Field.FieldLabel for="axis-name" required>
						{m.archive_axes_dialog_name_label()}
					</Field.FieldLabel>
					<Input id="axis-name" bind:value={axisFormName} required />
					<Field.FieldDescription>{m.archive_axes_dialog_name_help()}</Field.FieldDescription>
				</Field.Field>
				<Field.Field>
					<Field.FieldLabel for="axis-source">
						{m.archive_axes_dialog_source_label()}
					</Field.FieldLabel>
					<Select.Root type="single" bind:value={axisFormSource} required>
						<Select.Trigger id="axis-source" class="w-full">
							{sourceOptionLabel(axisFormSource)}
						</Select.Trigger>
						<Select.Content>
							{#each AXIS_SOURCES as source (source)}
								<Select.Item value={source}>{sourceOptionLabel(source)}</Select.Item>
							{/each}
						</Select.Content>
					</Select.Root>
				</Field.Field>
				{#if axisFormSource === 'dirLevel'}
					{#if dirLevels.status === 'loading'}
						<Spinner aria-label={m.archive_axes_dialog_dir_level_loading()} />
					{:else if dirLevelOptions.length > 0}
						<Field.FieldSet>
							<Field.FieldLegend id="axis-dir-level-legend" variant="label">
								{m.archive_axes_dialog_dir_level_choice_label()}
							</Field.FieldLegend>
							<RadioGroup.Root
								aria-labelledby="axis-dir-level-legend"
								bind:value={
									() => String(axisFormDirLevel ?? ''),
									(value) => (axisFormDirLevel = Number(value))
								}
							>
								{#each dirLevelOptions as option (option.level)}
									{@const id = `axis-dir-level-${option.level}`}
									<!-- 行全体を押せるラベルにする。読み上げ名は「第N階層」だけにし、値の例は説明として読ませる。 -->
									<Field.FieldLabel for={id} class="min-h-11">
										<Field.Field orientation="horizontal">
											<RadioGroup.Item
												value={String(option.level)}
												{id}
												aria-labelledby={`${id}-title`}
												aria-describedby={`${id}-description`}
											/>
											<Field.FieldContent>
												<Field.FieldTitle id={`${id}-title`}>
													{m.archive_axes_dialog_dir_level_option({ level: String(option.level) })}
												</Field.FieldTitle>
												<Field.FieldDescription id={`${id}-description`} class="wrap-anywhere">
													{option.summary === null
														? m.archive_axes_dialog_dir_level_option_no_items()
														: dirLevelSamplesText(option.summary)}
												</Field.FieldDescription>
											</Field.FieldContent>
										</Field.Field>
									</Field.FieldLabel>
								{/each}
							</RadioGroup.Root>
						</Field.FieldSet>
					{:else}
						{#if dirLevels.status === 'failed'}
							<p class="text-sm text-muted-foreground">
								{m.archive_axes_dialog_dir_level_fetch_failed()}
							</p>
						{:else}
							{@render noItemsNotice(m.archive_axes_dialog_dir_level_no_items())}
						{/if}
						<Field.Field>
							<Field.FieldLabel for="axis-dir-level" required>
								{m.archive_axes_dialog_dir_level_label()}
							</Field.FieldLabel>
							<Input
								id="axis-dir-level"
								type="number"
								min={1}
								bind:value={axisFormDirLevel}
								required
							/>
							<Field.FieldDescription>
								{m.archive_axes_dialog_dir_level_help()}
							</Field.FieldDescription>
						</Field.Field>
					{/if}
				{/if}
				<!-- 作成では既定 (絞り込みに出す・タイトルで省略しない) のまま作る。変えるのは編集で。 -->
				{#if editingAxisId !== null}
					<Field.Field orientation="horizontal">
						<Checkbox id="axis-filterable" bind:checked={axisFormFilterable} />
						<Field.FieldContent>
							<Field.FieldLabel for="axis-filterable">
								{m.archive_axes_dialog_filterable_label()}
							</Field.FieldLabel>
						</Field.FieldContent>
					</Field.Field>
					<Field.Field orientation="horizontal">
						<!-- 画面では反転して見せる。API・DB の optionalInTitle はそのまま (→ docs/archive.md「表示タイトル」)。 -->
						<Checkbox
							id="axis-optional-in-title"
							bind:checked={
								() => !axisFormOptionalInTitle, (checked) => (axisFormOptionalInTitle = !checked)
							}
						/>
						<Field.FieldContent>
							<Field.FieldLabel for="axis-optional-in-title">
								{m.archive_axes_dialog_optional_in_title_label()}
							</Field.FieldLabel>
							<Field.FieldDescription>
								{m.archive_axes_dialog_optional_in_title_help()}
							</Field.FieldDescription>
						</Field.FieldContent>
					</Field.Field>
				{/if}
			</Field.FieldGroup>
			{#if axisValuesWillBeCleared}
				<WarningBand>
					<p class="text-sm leading-relaxed">
						{m.archive_axes_dialog_values_cleared_warning()}
					</p>
				</WarningBand>
			{/if}
			<Dialog.Footer>
				<Button type="button" variant="outline" onclick={() => (axisDialogOpen = false)}>
					{m.action_cancel()}
				</Button>
				<LoadingButton
					type="submit"
					loading={axisSaving}
					disabled={dirLevelsPending || !axisForm.dirty}
				>
					{m.action_save()}
				</LoadingButton>
			</Dialog.Footer>
		</form>
	</Dialog.Content>
</Dialog.Root>

<ConfirmDialog
	bind:open={
		() => deletingAxis !== null,
		(open) => {
			if (!open) deletingAxis = null;
		}
	}
	title={m.archive_axes_delete_confirm_title()}
	busy={axisDeleting}
	onconfirm={handleDeleteAxis}
>
	{m.common_delete_confirm_description()}
</ConfirmDialog>

<AxesAiDialog bind:open={aiDialogOpen} contentId={data.contentId} onimported={invalidateAll} />

<ErrorDialog bind:open={errorDialog.open} message={errorDialog.message} title={errorDialog.title} />
