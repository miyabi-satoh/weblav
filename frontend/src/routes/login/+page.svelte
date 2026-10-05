<script lang="ts">
	import { goto, invalidateAll } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import { client } from '$lib/api/client';
	import { isApiTarget, safeRedirectTarget } from '$lib/auth';
	import { GENERIC_ERROR_MESSAGE, errorMessage } from '$lib/api/errors';
	import { ErrorDialogState } from '$lib/error-dialog-state.svelte';
	import * as m from '$lib/paraglide/messages.js';
	import * as Card from '$lib/components/ui/card';
	import * as Field from '$lib/components/ui/field';
	import { Input } from '$lib/components/ui/input';
	import PasswordInput from '$lib/components/password-input.svelte';
	import ErrorDialog from '$lib/components/error-dialog.svelte';
	import LoadingButton from '$lib/components/loading-button.svelte';
	import { pageTitle } from '$lib/page-title';
	import {
		centeredPageClass,
		centeredPageInnerClass,
		centeredPageCardClass,
		centeredPageFooterLinkClass
	} from '$lib/page-layout';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();
	let username = $state('');
	let password = $state('');
	let loading = $state(false);
	const errorDialog = new ErrorDialogState();

	async function handleSubmit(e: SubmitEvent) {
		e.preventDefault();
		if (loading) return;
		loading = true;

		try {
			const { error, response } = await client.POST('/api/v1/auth/login', {
				body: { username, password }
			});

			// サーバーの `message` は表示せず、`code` から表示文言を引く
			// (本文なしの 5xx や text/plain 応答でも errorMessage() が汎用文言に落とす)。
			if (!response.ok) {
				errorDialog.show(errorMessage(error));
				return;
			}

			// 保護ページから飛ばされてきた場合はそこへ戻す。値は自サイト内の絶対パスに
			// 限定する(オープンリダイレクト対策。→ `safeRedirectTarget`)。
			const target = safeRedirectTarget(page.url.searchParams.get('redirect'));
			if (target === null) {
				await goto(resolve('/'));
			} else if (isApiTarget(target)) {
				// ファイルを直接開こうとして 401 になった場合の戻り先。SvelteKit の
				// ルートではないので `goto` では解決できず、ブラウザに開かせる。
				//
				// インライン表示できない型 (docx・zip 等) はダウンロードが始まるだけで
				// この文書は遷移しない。ヘッダーが未ログインのまま残ると「ログインに
				// 失敗した」ように見えるので、先に画面の状態を更新しておく。
				await invalidateAll();
				window.location.assign(target);
			} else {
				// `target` はブラウザのURLから取った実パスであり、route id ではないため
				// `resolve()` は通さない。
				// eslint-disable-next-line svelte/no-navigation-without-resolve -- route id ではない実パスへの遷移 (上のコメント参照)
				await goto(target);
			}
		} catch {
			errorDialog.show(GENERIC_ERROR_MESSAGE());
		} finally {
			loading = false;
		}
	}
</script>

<svelte:head><title>{pageTitle(m.login_nav_label())}</title></svelte:head>

<div class={centeredPageClass}>
	<div class={centeredPageInnerClass}>
		<Card.Root class={centeredPageCardClass}>
			<Card.Header class="text-center">
				<Card.Title class="text-xl">{m.login_title()}</Card.Title>
			</Card.Header>
			<Card.Content>
				<form onsubmit={handleSubmit}>
					<Field.FieldGroup>
						<Field.Field>
							<Field.FieldLabel for="username">{m.login_username_label()}</Field.FieldLabel>
							<Input id="username" autocomplete="username" bind:value={username} required />
						</Field.Field>
						<Field.Field>
							<Field.FieldLabel for="password">{m.login_password_label()}</Field.FieldLabel>
							<PasswordInput
								id="password"
								autocomplete="current-password"
								bind:value={password}
								required
							/>
						</Field.Field>
						<Field.Field>
							<LoadingButton type="submit" class="w-full" {loading}>
								{m.login_submit()}
							</LoadingButton>
						</Field.Field>
					</Field.FieldGroup>
				</form>
			</Card.Content>
			<Card.Footer class="flex-col gap-4">
				<!-- リカバリコードはコードそのものが証明なので、LAN の端末にも出す (→ docs/access.md「リカバリコード」)。 -->
				<a href={resolve('/recover')} class={centeredPageFooterLinkClass}
					>{m.login_forgot_password_link()}</a
				>
				{#if data.setupRequired}
					<a href={resolve('/help/[slug]', { slug: 'setup' })} class={centeredPageFooterLinkClass}>
						{m.login_help_link()}
					</a>
				{/if}
			</Card.Footer>
		</Card.Root>
	</div>

	<ErrorDialog
		bind:open={errorDialog.open}
		message={errorDialog.message}
		title={m.login_error_title()}
	/>
</div>
