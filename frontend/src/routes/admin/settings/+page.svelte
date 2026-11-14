<script lang="ts">
	import { untrack } from 'svelte';
	import { toast } from 'svelte-sonner';
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import { client } from '#lib/api/client.ts';
	import { adminBackupHref, adminLogsHref } from '#lib/api/urls.ts';
	import { ErrorDialogState } from '#lib/error-dialog-state.svelte.ts';
	import { FormDirtyState } from '#lib/form-dirty.svelte.ts';
	import { gotoWithoutLeaveConfirm } from '#lib/leave-confirm.ts';
	import { LeaveGuard } from '#lib/leave-guard.svelte.ts';
	import { pageTitle } from '#lib/page-title.ts';
	import { canAddRoots } from '#lib/root-location.ts';
	import * as m from '#lib/paraglide/messages.js';
	import * as Field from '#lib/components/ui/field/index.ts';
	import { Button } from '#lib/components/ui/button/index.ts';
	import { Input } from '#lib/components/ui/input/index.ts';
	import { Switch } from '#lib/components/ui/switch/index.ts';
	import BackupRestore from '#lib/components/backup-restore.svelte';
	import AboutSection from '#lib/components/about-section.svelte';
	import ErrorDialog from '#lib/components/error-dialog.svelte';
	import IntegerInput from '#lib/components/integer-input.svelte';
	import LoadingButton from '#lib/components/loading-button.svelte';
	import LeaveConfirmDialog from '#lib/components/leave-confirm-dialog.svelte';
	import PatternField from '#lib/components/pattern-field.svelte';
	import type { components } from '#lib/api/schema.js';
	import type { PageProps } from './$types';
	import {
		adminPageClass,
		adminPageHeaderClass,
		pageHeadingTextClass,
		sectionCardClass,
		sectionGridClass
	} from '#lib/page-layout.ts';
	import { masonry } from '#lib/masonry.ts';

	let { data }: PageProps = $props();

	// サーバーの上限 (`src/api/site_settings.rs` の `MAX_CHARS`) と揃える。
	const MAX_LENGTH = 100;
	// サーバーの範囲 (`src/api/server_settings.rs`) と揃える。
	const MAX_PORT = 65535;
	const MAX_UPLOAD_MAX_SIZE_MB = 100 * 1024;
	const MAX_SESSION_EXPIRY_DAYS = 3650;
	// 3つの欄を、いちばん長い上限 (6 桁) と単位が収まる幅に揃える (→ docs/ui.md「見た目の決めごと」)。
	const serverNumberWidthClass = 'w-36';
	// 「既定値に戻す」を入力欄の右に並べる。狭い幅で収まらなければ下へ回す。
	const serverInputRowClass = 'flex flex-wrap items-center gap-2';

	// 保存後は応答の値で入れ直すので、読み込みは初回だけでよい。
	let siteName = $state(untrack(() => data.settings.siteName));
	let homeHeading = $state(untrack(() => data.settings.homeHeading));
	const siteDirty = new FormDirtyState(() => ({ siteName, homeHeading }));
	siteDirty.markPristine();

	// 取れなかったとき (null) は、区画に理由だけを出して入力欄を出さない。下の 0 は使われない。
	let server = $state(untrack(() => data.serverSettings));
	let port = $state(untrack(() => server?.saved.port ?? 0));
	// 上限なし (null) は欄では 0 と見せる (→ docs/ui.md「UI 全般」のサーバーの区画)。
	let uploadMaxSizeMb = $state(untrack(() => server?.saved.uploadMaxSizeMb ?? 0));
	// 送る値。上限なしなら null。
	let uploadCap = $derived(uploadMaxSizeMb === 0 ? null : uploadMaxSizeMb);
	let sessionExpiryDays = $state(untrack(() => server?.saved.sessionExpiryDays ?? 0));

	// 既定の値に戻しただけでは保存されないので、保存していない変更があることを保存ボタンの横に出す。
	// 欄を空にしただけでは値が変わらないので、欄の文字が値と違うことも変更に数える。
	let portInput = $state<IntegerInput>();
	let uploadInput = $state<IntegerInput>();
	let sessionInput = $state<IntegerInput>();
	const serverDirty = new FormDirtyState(() => ({
		port,
		uploadCap,
		sessionExpiryDays,
		portUnsettled: portInput?.isUnsettled() ?? false,
		uploadUnsettled: uploadInput?.isUnsettled() ?? false,
		sessionUnsettled: sessionInput?.isUnsettled() ?? false
	}));

	function uploadCapText(value: number | null | undefined): string {
		return value == null
			? m.admin_settings_upload_unlimited()
			: `${value} ${m.admin_settings_upload_unit()}`;
	}

	function sessionText(days: number): string {
		return days === 0
			? m.admin_settings_session_unlimited()
			: `${days} ${m.admin_settings_session_unit({ count: days })}`;
	}
	serverDirty.markPristine();

	// 戻すボタンは、戻すと消える。フォーカスが行き場を失わないよう、戻した入力欄へ移す。
	function resetField(input: IntegerInput | undefined, inputId: string, value: number) {
		input?.setValue(value);
		document.getElementById(inputId)?.focus();
	}

	// 表示しないもの。取れなかったとき (null) は、区画に理由だけを出す。
	let hidden = $state(untrack(() => data.hiddenSettings));
	let hiddenPatterns = $state(untrack(() => hidden?.patterns ?? ''));
	let windowsHidden = $state(untrack(() => hidden?.windowsHidden ?? true));
	const hiddenDirty = new FormDirtyState(() => ({ hiddenPatterns, windowsHidden }));
	hiddenDirty.markPristine();
	// 見える範囲を広げられるので、共有の追加と同じくサーバーの PC から開いたときだけ変えられる (→ docs/folders.md「登録と削除」)。
	let canEditHidden = $derived(canAddRoots(page.url.hostname));
	let savingHidden = $state(false);

	/** 空の行と前後の空白を除いた形。既定と比べるのに使う (サーバーも同じ形で保存する)。 */
	function normalizedPatterns(raw: string): string {
		return raw
			.split('\n')
			.map((line) => line.trim())
			.filter((line) => line !== '')
			.join('\n');
	}

	let logSettings = $state(untrack(() => data.logSettings));
	let switchingLog = $state(false);

	let savingSite = $state(false);
	let savingServer = $state(false);

	const errorDialog = new ErrorDialogState();

	// 保存していない変更を、別の画面へ移って失わないようにする (コンテンツの編集ページと同じ)。
	// 保存中に押した移動は、保存が済んでから続ける。
	const dirty = () => siteDirty.dirty || serverDirty.dirty || hiddenDirty.dirty;
	const leaveGuard = new LeaveGuard(dirty, () => savingSite || savingServer || savingHidden);

	async function handleSiteSubmit(e: SubmitEvent) {
		e.preventDefault();
		if (savingSite) return;
		savingSite = true;

		try {
			const result = await errorDialog.attempt(() =>
				client.PUT('/api/v1/admin/site-settings', { body: { siteName, homeHeading } })
			);
			const saved = result?.data;
			if (saved) {
				// サーバーが前後の空白を落とすので、保存された値で入れ直す。
				siteName = saved.siteName;
				homeHeading = saved.homeHeading;
				siteDirty.markPristine();
				toast.success(m.admin_settings_saved_toast());
			}
		} finally {
			savingSite = false;
		}
		// ほかの保存が続いていれば、止めた移動はその保存の後に任せる。
		if (!savingServer && !savingHidden)
			await leaveGuard.resumeAfterSave(() => dirty() || errorDialog.open);
	}

	async function handleServerSubmit(e: SubmitEvent) {
		e.preventDefault();
		if (savingServer) return;
		savingServer = true;

		try {
			const result = await errorDialog.attempt(() =>
				client.PUT('/api/v1/admin/server-settings', {
					body: { port, uploadMaxSizeMb: uploadCap, sessionExpiryDays }
				})
			);
			const saved = result?.data;
			if (saved) {
				server = saved;
				uploadMaxSizeMb = saved.saved.uploadMaxSizeMb ?? 0;
				serverDirty.markPristine();
				toast.success(m.admin_settings_server_saved_toast());
			}
		} finally {
			savingServer = false;
		}
		if (!savingSite && !savingHidden)
			await leaveGuard.resumeAfterSave(() => dirty() || errorDialog.open);
	}

	async function handleHiddenSubmit(e: SubmitEvent) {
		e.preventDefault();
		if (savingHidden || !hidden) return;
		savingHidden = true;

		try {
			const patterns = normalizedPatterns(hiddenPatterns);
			// 既定と同じなら既定のままにする。版を上げて既定が増えたときに届くように (→ `src/api/hidden.rs`)。
			const body = {
				patterns: patterns === hidden.defaultPatterns ? null : patterns,
				windowsHidden: hidden.windowsHidden == null ? undefined : windowsHidden
			};
			const result = await errorDialog.attempt(() =>
				client.PUT('/api/v1/admin/hidden-settings', { body })
			);
			const saved = result?.data;
			if (saved) {
				hidden = saved;
				hiddenPatterns = saved.patterns;
				windowsHidden = saved.windowsHidden ?? true;
				hiddenDirty.markPristine();
				toast.success(m.common_section_saved_toast({ section: m.admin_settings_hidden_heading() }));
			}
		} finally {
			savingHidden = false;
		}
		if (!savingSite && !savingServer)
			await leaveGuard.resumeAfterSave(() => dirty() || errorDialog.open);
	}

	// 保存ボタンを置かず、切り替えたらすぐ送る。起動し直さずに効く設定なので。
	async function setVerbose(verbose: boolean) {
		if (!logSettings || switchingLog) return;
		switchingLog = true;

		try {
			const result = await errorDialog.attempt(() =>
				client.PUT('/api/v1/admin/log-settings', { body: { verbose } })
			);
			const saved = result?.data;
			if (saved) {
				logSettings = saved;
			}
		} finally {
			switchingLog = false;
		}
	}

	// ファイル名に作った日時を利用者の時刻で入れる。サーバーは名前を付けない (→ docs/access.md「バックアップとリストア」)。
	async function downloadBackup() {
		// ダウンロードはブラウザーが直接開くので、確かめ直しが要ると断られても画面では受けられない。
		// 確かめ直しは引換券を受け取るときに済ませる (→ docs/access.md「操作の前のパスワードの確認」)。
		const issued = await errorDialog.attempt(() => client.POST('/api/v1/admin/backup/ticket'));
		if (!issued?.data) return;
		const now = new Date();
		const pad = (n: number) => String(n).padStart(2, '0');
		const stamp = `${now.getFullYear()}${pad(now.getMonth() + 1)}${pad(now.getDate())}-${pad(now.getHours())}${pad(now.getMinutes())}`;
		const link = document.createElement('a');
		link.href = adminBackupHref(issued.data.ticket);
		link.download = `weblav-backup-${stamp}.zip`;
		link.click();
	}

	function stageBackup(body: FormData) {
		return client.POST('/api/v1/admin/backup/restore/stage', {
			// openapi-fetch は FormData をそのまま送るが、生成された型はスキーマの形のままなので合わせる。
			body: body as unknown as components['schemas']['StageBackupRequest']
		});
	}

	function restoreBackup(id: string) {
		return client.POST('/api/v1/admin/backup/restore', { body: { id } });
	}

	// 戻すと全員のセッションが消える。自分のも消えているので、ログインし直してもらう。
	async function handleRestored() {
		toast.success(m.backup_restored_toast());
		// 戻した後は書きかけの値に意味が無いので、離れるときの確認を出さない。
		await gotoWithoutLeaveConfirm(resolve('login'));
	}
</script>

<svelte:head><title>{pageTitle(m.admin_settings_nav_title())}</title></svelte:head>

<!-- 保存した値が動いている値と違う (次の起動から効く、またはポートをずらして起動した) ときに添える。 -->
{#snippet runningValue(saved: number | string, running: number | string)}
	{#if saved !== running}
		<Field.FieldDescription
			>{m.admin_settings_running_value({ value: running })}</Field.FieldDescription
		>
	{/if}
{/snippet}

<!-- 既定の値へ戻すボタン。変えてある項目 (欄を空にしたときも含む) にだけ、入力欄の右に出す。印や既定の値の文を足さず、ボタンが出ていることで変えてあると分かる形にする。
     戻すのは入力欄の値だけで、保存は区画の保存ボタンで行う。 -->
{#snippet defaultValue(name: string, changed: boolean, reset: () => void)}
	{#if changed}
		<Button
			type="button"
			variant="outline"
			size="xs"
			aria-label={m.admin_settings_reset_button_label({ name })}
			onclick={reset}>{m.admin_settings_reset_button()}</Button
		>
	{/if}
{/snippet}

<div class={adminPageClass}>
	<div class={adminPageHeaderClass}>
		<h1 class={pageHeadingTextClass}>{m.admin_settings_nav_title()}</h1>
	</div>

	<!-- 区画はカードにし、広い幅では石垣に並べる (→ docs/ui.md「UI 全般」の「サイト設定の区画」)。
	     保存は区画ごと。サーバーの区画だけが次の起動から効くため。 -->
	<div class={sectionGridClass} {@attach masonry}>
		<form
			onsubmit={handleSiteSubmit}
			aria-labelledby="home-section-heading"
			class={sectionCardClass}
		>
			<h2 id="home-section-heading" class="text-base font-semibold">
				{m.admin_settings_home_section_heading()}
			</h2>
			<Field.FieldGroup>
				<Field.Field>
					<Field.FieldLabel for="site-name">{m.admin_settings_site_name_label()}</Field.FieldLabel>
					<Input id="site-name" bind:value={siteName} maxlength={MAX_LENGTH} />
				</Field.Field>
				<Field.Field>
					<Field.FieldLabel for="home-heading"
						>{m.admin_settings_home_heading_label()}</Field.FieldLabel
					>
					<Input id="home-heading" bind:value={homeHeading} maxlength={MAX_LENGTH} />
				</Field.Field>
			</Field.FieldGroup>
			<div>
				<LoadingButton type="submit" loading={savingSite}>{m.action_save()}</LoadingButton>
			</div>
		</form>

		<form
			onsubmit={handleServerSubmit}
			aria-labelledby="server-section-heading"
			class={sectionCardClass}
		>
			<div class="flex flex-col gap-2">
				<h2 id="server-section-heading" class="text-base font-semibold">
					{m.admin_settings_server_heading()}
				</h2>
				<p class="text-sm text-muted-foreground">{m.admin_settings_server_description()}</p>
			</div>
			{#if server}
				<Field.FieldGroup>
					<Field.Field>
						<Field.FieldLabel for="server-port" required
							>{m.admin_settings_port_label()}</Field.FieldLabel
						>
						<div class={serverInputRowClass}>
							<IntegerInput
								id="server-port"
								class={serverNumberWidthClass}
								min={1}
								max={MAX_PORT}
								bind:value={port}
								bind:this={portInput}
								required
							/>
							{@render defaultValue(
								m.admin_settings_port_label(),
								port !== server.defaults.port || (portInput?.isUnsettled() ?? false),
								() => resetField(portInput, 'server-port', server?.defaults.port ?? port)
							)}
						</div>
						<Field.FieldDescription>{m.admin_settings_port_description()}</Field.FieldDescription>
						{@render runningValue(server.saved.port, server.running.port)}
					</Field.Field>
					<Field.Field>
						<Field.FieldLabel for="server-upload" required
							>{m.admin_settings_upload_label()}</Field.FieldLabel
						>
						<div class={serverInputRowClass}>
							<IntegerInput
								id="server-upload"
								class={serverNumberWidthClass}
								min={0}
								max={MAX_UPLOAD_MAX_SIZE_MB}
								unit={m.admin_settings_upload_unit()}
								bind:value={uploadMaxSizeMb}
								bind:this={uploadInput}
								required
							/>
							{@render defaultValue(
								m.admin_settings_upload_label(),
								uploadCap !== (server.defaults.uploadMaxSizeMb ?? null) ||
									(uploadInput?.isUnsettled() ?? false),
								() =>
									resetField(uploadInput, 'server-upload', server?.defaults.uploadMaxSizeMb ?? 0)
							)}
						</div>
						<Field.FieldDescription>{m.admin_settings_upload_description()}</Field.FieldDescription>
						{@render runningValue(
							uploadCapText(server.saved.uploadMaxSizeMb),
							uploadCapText(server.running.uploadMaxSizeMb)
						)}
					</Field.Field>
					<Field.Field>
						<Field.FieldLabel for="server-session" required
							>{m.admin_settings_session_label()}</Field.FieldLabel
						>
						<div class={serverInputRowClass}>
							<IntegerInput
								id="server-session"
								class={serverNumberWidthClass}
								min={0}
								max={MAX_SESSION_EXPIRY_DAYS}
								unit={m.admin_settings_session_unit({ count: sessionExpiryDays })}
								bind:value={sessionExpiryDays}
								bind:this={sessionInput}
								required
							/>
							{@render defaultValue(
								m.admin_settings_session_label(),
								sessionExpiryDays !== server.defaults.sessionExpiryDays ||
									(sessionInput?.isUnsettled() ?? false),
								() =>
									resetField(
										sessionInput,
										'server-session',
										server?.defaults.sessionExpiryDays ?? sessionExpiryDays
									)
							)}
						</div>
						<Field.FieldDescription>{m.admin_settings_session_description()}</Field.FieldDescription
						>
						{@render runningValue(
							sessionText(server.saved.sessionExpiryDays),
							sessionText(server.running.sessionExpiryDays)
						)}
					</Field.Field>
				</Field.FieldGroup>
				<div class="flex flex-wrap items-center gap-x-3 gap-y-1">
					<LoadingButton type="submit" loading={savingServer}>{m.action_save()}</LoadingButton>
					{#if serverDirty.dirty}
						<p class="text-sm text-muted-foreground">{m.admin_settings_server_unsaved()}</p>
					{/if}
				</div>
			{:else}
				<p class="text-sm text-destructive">{data.serverSettingsError}</p>
			{/if}
		</form>

		<form
			onsubmit={handleHiddenSubmit}
			aria-labelledby="hidden-section-heading"
			class={sectionCardClass}
		>
			<div class="flex flex-col gap-2">
				<h2 id="hidden-section-heading" class="text-base font-semibold">
					{m.admin_settings_hidden_heading()}
				</h2>
				<p class="text-sm text-muted-foreground">{m.admin_settings_hidden_description()}</p>
			</div>
			{#if hidden}
				<Field.FieldGroup>
					<Field.Field>
						<Field.FieldTitle>{m.admin_settings_hidden_fixed_label()}</Field.FieldTitle>
						<ul class="list-disc pl-5 text-sm">
							<li>{m.admin_settings_hidden_fixed_links()}</li>
							<li>{m.admin_settings_hidden_fixed_own()}</li>
						</ul>
					</Field.Field>
					<div class="flex flex-col gap-2">
						<PatternField
							label={m.admin_settings_hidden_patterns_label()}
							help={m.admin_settings_hidden_patterns_help()}
							emptyText={m.contents_form_exclude_patterns_empty()}
							readonly={!canEditHidden}
							bind:value={hiddenPatterns}
						/>
						{#if canEditHidden}
							<div>
								{@render defaultValue(
									m.admin_settings_hidden_patterns_label(),
									normalizedPatterns(hiddenPatterns) !== hidden.defaultPatterns,
									() => (hiddenPatterns = hidden?.defaultPatterns ?? hiddenPatterns)
								)}
							</div>
						{/if}
					</div>
					{#if hidden.windowsHidden != null}
						<Field.Field orientation="horizontal" data-disabled={!canEditHidden}>
							<Field.FieldLabel for="hidden-windows"
								>{m.admin_settings_hidden_windows_label()}</Field.FieldLabel
							>
							<Switch id="hidden-windows" bind:checked={windowsHidden} disabled={!canEditHidden} />
						</Field.Field>
					{/if}
				</Field.FieldGroup>
				{#if canEditHidden}
					<div>
						<LoadingButton type="submit" loading={savingHidden}>{m.action_save()}</LoadingButton>
					</div>
				{:else}
					<p class="text-sm text-muted-foreground">{m.admin_settings_hidden_local_only()}</p>
				{/if}
			{:else}
				<p class="text-sm text-destructive">{m.admin_settings_hidden_fetch_failed()}</p>
			{/if}
		</form>

		<section aria-labelledby="log-section-heading" class={sectionCardClass}>
			<h2 id="log-section-heading" class="text-base font-semibold">
				{m.admin_settings_log_heading()}
			</h2>
			{#if logSettings}
				<Field.FieldGroup>
					<Field.Field orientation="horizontal" data-disabled={switchingLog}>
						<Field.FieldContent>
							<Field.FieldLabel for="log-verbose"
								>{m.admin_settings_log_verbose_label()}</Field.FieldLabel
							>
							<Field.FieldDescription
								>{m.admin_settings_log_verbose_description()}</Field.FieldDescription
							>
						</Field.FieldContent>
						<Switch
							id="log-verbose"
							bind:checked={() => logSettings?.verbose ?? false, (v) => setVerbose(v)}
							disabled={switchingLog}
						/>
					</Field.Field>
					<Field.Field>
						{#if logSettings.fileOutput}
							<div>
								<!-- ファイルとして保存させる。開いた画面を離れずに済むよう download を付ける。 -->
								<Button variant="outline" href={adminLogsHref()} download
									>{m.admin_settings_log_download()}</Button
								>
							</div>
						{:else}
							<p class="text-sm text-muted-foreground">{m.admin_settings_log_stdout()}</p>
						{/if}
					</Field.Field>
				</Field.FieldGroup>
			{:else}
				<p class="text-sm text-destructive">{m.admin_settings_log_fetch_failed()}</p>
			{/if}
		</section>

		<section aria-labelledby="backup-section-heading" class={sectionCardClass}>
			<h2 id="backup-section-heading" class="text-base font-semibold">
				{m.backup_heading()}
			</h2>
			<div class="flex flex-col gap-2">
				<div>
					<Button variant="outline" onclick={downloadBackup}>{m.backup_download()}</Button>
				</div>
				<p class="text-sm text-muted-foreground">{m.backup_download_description()}</p>
			</div>
			<div class="flex flex-col gap-2">
				<BackupRestore stage={stageBackup} restore={restoreBackup} onrestored={handleRestored}>
					{#snippet confirmNote()}
						{m.backup_restore_confirm_relogin()}
					{/snippet}
				</BackupRestore>
			</div>
		</section>

		<!-- 版は問い合わせのときに読み上げてもらう。取れなければ区画ごと出さない (リンクだけ残しても用が足りないため)。 -->
		{#if data.health}
			<section aria-labelledby="about-section-heading" class={sectionCardClass}>
				<h2 id="about-section-heading" class="text-base font-semibold">
					{m.admin_settings_about_heading()}
				</h2>
				<AboutSection version={data.health.version} build={data.health.build ?? null} />
			</section>
		{/if}
	</div>
</div>

<LeaveConfirmDialog
	guard={leaveGuard}
	ondiscard={() => {
		siteDirty.markPristine();
		serverDirty.markPristine();
	}}
/>

<ErrorDialog bind:open={errorDialog.open} message={errorDialog.message} title={errorDialog.title} />
