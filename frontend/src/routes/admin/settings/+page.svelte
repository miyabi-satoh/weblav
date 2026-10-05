<script lang="ts">
	import { untrack } from 'svelte';
	import { toast } from 'svelte-sonner';
	import { goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { client } from '$lib/api/client';
	import { adminBackupHref, adminLogsHref } from '$lib/api/urls';
	import { GENERIC_ERROR_MESSAGE, errorMessage } from '$lib/api/errors';
	import { ErrorDialogState } from '$lib/error-dialog-state.svelte';
	import { pageTitle } from '$lib/page-title';
	import * as m from '$lib/paraglide/messages.js';
	import * as Field from '$lib/components/ui/field';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Switch } from '$lib/components/ui/switch';
	import BackupRestore from '$lib/components/backup-restore.svelte';
	import ProSection from '$lib/components/pro-section.svelte';
	import ErrorDialog from '$lib/components/error-dialog.svelte';
	import LoadingButton from '$lib/components/loading-button.svelte';
	import type { components } from '$lib/api/schema';
	import type { PageProps } from './$types';
	import { adminPageClass, adminPageHeaderClass, pageHeadingTextClass } from '$lib/page-layout';

	let { data }: PageProps = $props();

	// サーバーの上限 (`src/api/site_settings.rs` の `MAX_CHARS`) と揃える。
	const MAX_LENGTH = 100;
	// サーバーの範囲 (`src/api/server_settings.rs`) と揃える。
	const MAX_PORT = 65535;
	const MAX_UPLOAD_MAX_SIZE_MB = 100 * 1024;
	const MAX_SESSION_EXPIRY_DAYS = 3650;

	// 保存後は応答の値で入れ直すので、読み込みは初回だけでよい。
	let siteName = $state(untrack(() => data.settings.siteName));
	let homeHeading = $state(untrack(() => data.settings.homeHeading));

	// 取れなかったとき (null) は、区画に理由だけを出して入力欄を出さない。下の 0 は使われない。
	let server = $state(untrack(() => data.serverSettings));
	let port = $state(untrack(() => server?.saved.port ?? 0));
	let uploadMaxSizeMb = $state(untrack(() => server?.saved.uploadMaxSizeMb ?? 0));
	let sessionExpiryDays = $state(untrack(() => server?.saved.sessionExpiryDays ?? 0));

	let logSettings = $state(untrack(() => data.logSettings));
	let switchingLog = $state(false);

	let savingSite = $state(false);
	let savingServer = $state(false);

	const errorDialog = new ErrorDialogState();

	async function handleSiteSubmit(e: SubmitEvent) {
		e.preventDefault();
		if (savingSite) return;
		savingSite = true;

		try {
			const {
				data: saved,
				error,
				response
			} = await client.PUT('/api/v1/admin/site-settings', {
				body: { siteName, homeHeading }
			});
			if (!response.ok || !saved) {
				errorDialog.show(errorMessage(error));
				return;
			}
			// サーバーが前後の空白を落とすので、保存された値で入れ直す。
			siteName = saved.siteName;
			homeHeading = saved.homeHeading;
			toast.success(m.admin_settings_saved_toast());
		} catch {
			errorDialog.show(GENERIC_ERROR_MESSAGE());
		} finally {
			savingSite = false;
		}
	}

	async function handleServerSubmit(e: SubmitEvent) {
		e.preventDefault();
		if (savingServer) return;
		savingServer = true;

		try {
			const {
				data: saved,
				error,
				response
			} = await client.PUT('/api/v1/admin/server-settings', {
				body: { port, uploadMaxSizeMb, sessionExpiryDays }
			});
			if (!response.ok || !saved) {
				errorDialog.show(errorMessage(error));
				return;
			}
			server = saved;
			toast.success(m.admin_settings_server_saved_toast());
		} catch {
			errorDialog.show(GENERIC_ERROR_MESSAGE());
		} finally {
			savingServer = false;
		}
	}

	// 保存ボタンを置かず、切り替えたらすぐ送る。起動し直さずに効く設定なので。
	async function setVerbose(verbose: boolean) {
		if (!logSettings || switchingLog) return;
		switchingLog = true;

		try {
			const {
				data: saved,
				error,
				response
			} = await client.PUT('/api/v1/admin/log-settings', { body: { verbose } });
			if (!response.ok || !saved) {
				errorDialog.show(errorMessage(error));
				return;
			}
			logSettings = saved;
		} catch {
			errorDialog.show(GENERIC_ERROR_MESSAGE());
		} finally {
			switchingLog = false;
		}
	}

	// ファイル名に作った日時を利用者の時刻で入れる。サーバーは名前を付けない (→ docs/access.md「バックアップとリストア」)。
	function downloadBackup() {
		const now = new Date();
		const pad = (n: number) => String(n).padStart(2, '0');
		const stamp = `${now.getFullYear()}${pad(now.getMonth() + 1)}${pad(now.getDate())}-${pad(now.getHours())}${pad(now.getMinutes())}`;
		const link = document.createElement('a');
		link.href = adminBackupHref();
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
		await goto(resolve('/login'));
	}
</script>

<svelte:head><title>{pageTitle(m.admin_settings_title())}</title></svelte:head>

<!-- 保存した値が動いている値と違う (次の起動から効く、またはポートをずらして起動した) ときに添える。 -->
{#snippet runningValue(saved: number, running: number)}
	{#if saved !== running}
		<Field.FieldDescription
			>{m.admin_settings_running_value({ value: running })}</Field.FieldDescription
		>
	{/if}
{/snippet}

<div class={adminPageClass}>
	<div class={adminPageHeaderClass}>
		<h1 class={pageHeadingTextClass}>{m.admin_settings_title()}</h1>
	</div>

	<!-- 区画の間は 48px 空け、その中ほどに罫線を引く (→ docs/ui.md「UI 全般」の「余白と区切り」)。
	     保存は区画ごと。サーバーの区画だけが次の起動から効くため。 -->
	<div class="flex max-w-xl flex-col">
		<form
			onsubmit={handleSiteSubmit}
			aria-labelledby="home-section-heading"
			class="flex flex-col gap-6 border-b pb-6"
		>
			<h2 id="home-section-heading" class="text-base font-semibold">
				{m.admin_settings_home_section_heading()}
			</h2>
			<Field.FieldGroup>
				<Field.Field>
					<Field.FieldLabel for="site-name">{m.admin_settings_site_name_label()}</Field.FieldLabel>
					<Input id="site-name" bind:value={siteName} maxlength={MAX_LENGTH} />
					<Field.FieldDescription>{m.admin_settings_site_name_description()}</Field.FieldDescription
					>
				</Field.Field>
				<Field.Field>
					<Field.FieldLabel for="home-heading"
						>{m.admin_settings_home_heading_label()}</Field.FieldLabel
					>
					<Input id="home-heading" bind:value={homeHeading} maxlength={MAX_LENGTH} />
					<Field.FieldDescription
						>{m.admin_settings_home_heading_description()}</Field.FieldDescription
					>
				</Field.Field>
			</Field.FieldGroup>
			<div>
				<LoadingButton type="submit" loading={savingSite}>{m.action_save()}</LoadingButton>
			</div>
		</form>

		<form
			onsubmit={handleServerSubmit}
			aria-labelledby="server-section-heading"
			class="flex flex-col gap-6 pt-6"
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
						<Input
							id="server-port"
							type="number"
							min={1}
							max={MAX_PORT}
							bind:value={port}
							required
						/>
						<Field.FieldDescription>{m.admin_settings_port_description()}</Field.FieldDescription>
						{@render runningValue(server.saved.port, server.running.port)}
					</Field.Field>
					<Field.Field>
						<Field.FieldLabel for="server-upload" required
							>{m.admin_settings_upload_label()}</Field.FieldLabel
						>
						<Input
							id="server-upload"
							type="number"
							min={1}
							max={MAX_UPLOAD_MAX_SIZE_MB}
							bind:value={uploadMaxSizeMb}
							required
						/>
						<Field.FieldDescription>{m.admin_settings_upload_description()}</Field.FieldDescription>
						{@render runningValue(server.saved.uploadMaxSizeMb, server.running.uploadMaxSizeMb)}
					</Field.Field>
					<Field.Field>
						<Field.FieldLabel for="server-session" required
							>{m.admin_settings_session_label()}</Field.FieldLabel
						>
						<Input
							id="server-session"
							type="number"
							min={1}
							max={MAX_SESSION_EXPIRY_DAYS}
							bind:value={sessionExpiryDays}
							required
						/>
						<Field.FieldDescription>{m.admin_settings_session_description()}</Field.FieldDescription
						>
						{@render runningValue(server.saved.sessionExpiryDays, server.running.sessionExpiryDays)}
					</Field.Field>
				</Field.FieldGroup>
				<div>
					<LoadingButton type="submit" loading={savingServer}>{m.action_save()}</LoadingButton>
				</div>
			{:else}
				<p class="text-sm text-destructive">{data.serverSettingsError}</p>
			{/if}
		</form>

		<section aria-labelledby="log-section-heading" class="mt-6 flex flex-col gap-6 border-t pt-6">
			<h2 id="log-section-heading" class="text-base font-semibold">
				{m.admin_settings_log_heading()}
			</h2>
			{#if logSettings}
				<Field.FieldGroup>
					<Field.Field orientation="horizontal">
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
							<Field.FieldDescription
								>{m.admin_settings_log_download_description()}</Field.FieldDescription
							>
						{:else}
							<p class="text-sm text-muted-foreground">{m.admin_settings_log_stdout()}</p>
						{/if}
					</Field.Field>
				</Field.FieldGroup>
			{:else}
				<p class="text-sm text-destructive">{m.admin_settings_log_fetch_failed()}</p>
			{/if}
		</section>

		<section
			aria-labelledby="backup-section-heading"
			class="mt-6 flex flex-col gap-6 border-t pt-6"
		>
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
				<p class="text-sm text-muted-foreground">{m.backup_restore_description()}</p>
			</div>
		</section>

		<section aria-labelledby="pro-section-heading" class="mt-6 flex flex-col gap-6 border-t pt-6">
			<h2 id="pro-section-heading" class="text-base font-semibold">
				{m.admin_settings_pro_heading()}
			</h2>
			{#if data.pro}
				<ProSection initial={data.pro} />
			{:else}
				<p class="text-sm text-destructive">{m.admin_settings_pro_fetch_failed()}</p>
			{/if}
		</section>
	</div>
</div>

<ErrorDialog bind:open={errorDialog.open} message={errorDialog.message} title={errorDialog.title} />
