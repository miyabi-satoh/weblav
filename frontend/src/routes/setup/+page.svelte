<script lang="ts">
	import { toast } from 'svelte-sonner';
	import { goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { client } from '$lib/api/client';
	import { GENERIC_ERROR_MESSAGE, errorMessage } from '$lib/api/errors';
	import { ErrorDialogState } from '$lib/error-dialog-state.svelte';
	import * as m from '$lib/paraglide/messages.js';
	import * as Card from '$lib/components/ui/card';
	import * as Field from '$lib/components/ui/field';
	import { Input } from '$lib/components/ui/input';
	import PasswordInput from '$lib/components/password-input.svelte';
	import type { components } from '$lib/api/schema';
	import BackupRestore from '$lib/components/backup-restore.svelte';
	import ErrorDialog from '$lib/components/error-dialog.svelte';
	import LoadingButton from '$lib/components/loading-button.svelte';
	import RecoveryCodePanel from '$lib/components/recovery-code-panel.svelte';
	import { pageTitle } from '$lib/page-title';
	import {
		centeredPageClass,
		centeredPageInnerClass,
		centeredPageCardClass
	} from '$lib/page-layout';

	let { data } = $props();

	let username = $state('');
	let password = $state('');
	let passwordConfirm = $state('');
	let loading = $state(false);
	/** 管理者を作れたら、その人のリカバリコードを見せる (→ docs/access.md「リカバリコード」)。 */
	let recoveryCode = $state<string | null>(null);
	/** 控えに書くユーザー名。サーバーが実際に作った名前 (前後の空白を落としたもの) を使う。 */
	let accountName = $state('');
	const errorDialog = new ErrorDialogState();
	const passwordMismatch = $derived(passwordConfirm !== '' && password !== passwordConfirm);

	async function handleSubmit(e: SubmitEvent) {
		e.preventDefault();
		if (loading || passwordMismatch) return;
		loading = true;

		try {
			const {
				data: created,
				error,
				response
			} = await client.POST('/api/v1/setup/admin', {
				body: { token: data.token, username, password }
			});

			if (!response.ok) {
				// 入力中にトークンが期限切れ・使い切りになったら、開いたときと同じ案内に寄せる
				// (「見つかりません」では、トレイから開き直せばよいことが伝わらない)。
				// それ以外は、サーバーの `message` ではなく `code` から文言を引く (ログインと同じ)。
				errorDialog.show(
					response.status === 404 ? m.setup_invalid_link_description() : errorMessage(error)
				);
				return;
			}

			// 作成と同時にログイン済みになっている (→ docs/access.md「初回セットアップ」)。
			// リカバリコードを保管してもらってから管理画面へ送る。
			if (created) {
				accountName = created.user.username;
				recoveryCode = created.recoveryCode;
				return;
			}
			await goto(resolve('/admin/contents'));
		} catch {
			errorDialog.show(GENERIC_ERROR_MESSAGE());
		} finally {
			loading = false;
		}
	}

	// 画面が移れば、ルートの `+layout.ts` がログイン状態を取り直す。
	async function handleRecoveryCodeKept() {
		await goto(resolve('/admin/contents'));
	}

	// 管理者を作る代わりに、バックアップから戻す (→ docs/access.md「バックアップとリストア」)。
	function stageBackup(body: FormData) {
		return client.POST('/api/v1/setup/restore/stage', {
			params: { header: { 'x-setup-token': data.token } },
			// openapi-fetch は FormData をそのまま送るが、生成された型はスキーマの形のままなので合わせる。
			body: body as unknown as components['schemas']['StageBackupRequest']
		});
	}

	function restoreBackup(id: string) {
		return client.POST('/api/v1/setup/restore', { body: { token: data.token, id } });
	}

	// 戻してもログインはさせない。戻したデータの管理者でログインしてもらう。
	async function handleRestored() {
		toast.success(m.setup_restored_toast());
		await goto(resolve('/login'));
	}
</script>

<svelte:head><title>{pageTitle(m.setup_title())}</title></svelte:head>

<div class={centeredPageClass}>
	<div class={centeredPageInnerClass}>
		<Card.Root class={centeredPageCardClass}>
			<Card.Header class="text-center">
				<Card.Title class="text-xl">{m.setup_title()}</Card.Title>
				<Card.Description>
					{#if recoveryCode !== null}
						{m.setup_recovery_code_description()}
					{:else}
						{data.valid ? m.setup_description() : m.setup_invalid_link_description()}
					{/if}
				</Card.Description>
			</Card.Header>
			{#if recoveryCode !== null}
				<Card.Content>
					<RecoveryCodePanel
						code={recoveryCode}
						username={accountName}
						onconfirm={handleRecoveryCodeKept}
					/>
				</Card.Content>
			{:else if data.valid}
				<Card.Content>
					<form onsubmit={handleSubmit}>
						<Field.FieldGroup>
							<Field.Field>
								<Field.FieldLabel for="username" required
									>{m.setup_username_label()}</Field.FieldLabel
								>
								<Input id="username" autocomplete="username" bind:value={username} required />
							</Field.Field>
							<Field.Field>
								<Field.FieldLabel for="password" required
									>{m.setup_password_label()}</Field.FieldLabel
								>
								<PasswordInput
									id="password"
									autocomplete="new-password"
									bind:value={password}
									required
								/>
							</Field.Field>
							<Field.Field>
								<Field.FieldLabel for="password-confirm" required
									>{m.setup_password_confirm_label()}</Field.FieldLabel
								>
								<PasswordInput
									id="password-confirm"
									autocomplete="new-password"
									aria-invalid={passwordMismatch}
									bind:value={passwordConfirm}
									required
								/>
								{#if passwordMismatch}
									<Field.FieldError>{m.common_password_mismatch()}</Field.FieldError>
								{/if}
							</Field.Field>
							<Field.Field>
								<LoadingButton type="submit" class="w-full" {loading} disabled={passwordMismatch}>
									{m.setup_submit()}
								</LoadingButton>
							</Field.Field>
						</Field.FieldGroup>
					</form>
					<div class="mt-6 flex flex-col gap-2 border-t pt-6">
						<p class="text-sm text-muted-foreground">{m.setup_restore_description()}</p>
						<!-- トークンの失効も受け取ったものの期限切れも 404 になる。どちらもトレイから開き直せば済む。 -->
						<BackupRestore
							stage={stageBackup}
							restore={restoreBackup}
							onrestored={handleRestored}
							notFoundMessage={m.setup_invalid_link_description()}
						/>
					</div>
				</Card.Content>
			{/if}
		</Card.Root>
	</div>

	<ErrorDialog
		bind:open={errorDialog.open}
		message={errorDialog.message}
		title={m.setup_error_title()}
	/>
</div>
