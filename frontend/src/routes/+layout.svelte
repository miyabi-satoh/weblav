<script lang="ts">
	import { onMount } from 'svelte';
	import { resolve } from '$app/paths';
	import { goto, invalidateAll } from '$app/navigation';
	import { page } from '$app/state';
	import * as m from '$lib/paraglide/messages.js';
	import { getLocale, getTextDirection } from '$lib/paraglide/runtime';
	import './layout.css';
	import favicon from '$lib/assets/favicon.svg';
	import { ModeWatcher } from 'mode-watcher';
	import { client } from '$lib/api/client';
	import { GENERIC_ERROR_MESSAGE, errorMessage, messageForCode } from '$lib/api/errors';
	import { ErrorDialogState } from '$lib/error-dialog-state.svelte';
	import { headerTextActionClass } from '$lib/header-action';
	import ErrorDialog from '$lib/components/error-dialog.svelte';
	import ModeToggle from '$lib/components/mode-toggle.svelte';
	import LanguageToggle from '$lib/components/language-toggle.svelte';
	import ConnectionInfoDialog from '$lib/components/connection-info-dialog.svelte';
	import UserMenu from '$lib/components/user-menu.svelte';
	import AudioPlayerBar from '$lib/components/audio-player-bar.svelte';
	import FileViewer from '$lib/components/file-viewer.svelte';
	import NavigationProgress from '$lib/components/navigation-progress.svelte';
	import { Toaster } from '$lib/components/ui/sonner';
	import { nowPlaying } from '$lib/now-playing.svelte';

	let { children } = $props();

	const errorDialog = new ErrorDialogState();

	let playerHeight = $state(0);

	// ssr = false のため `hooks.server.ts` の transformPageChunk はビルド時に1回しか
	// 走らず、`<html lang>`/`dir` はbaseLocale固定になる(JS無効時の既定値としては残す)。
	// 実際のロケールはここで起動時に1度反映する。`setLocale` は既定でリロードするので
	// ロケール変更時の追随はそちらに任せてよい。
	onMount(() => {
		document.documentElement.lang = getLocale();
		document.documentElement.dir = getTextDirection();
	});

	// ブラウザが直接開いたファイルの取得に失敗すると、サーバーはここへ理由を載せて
	// 送ってくる (→ `src/api/browser.rs`)。生の JSON をタブに出さないため。
	$effect(() => {
		const code = page.url.searchParams.get('error');
		if (code === null) return;
		errorDialog.show(messageForCode(code));
		// 読み込み直しやブックマークで同じ知らせが蘇らないよう、クエリを落とす。
		// 履歴は増やさない (戻るボタンでエラーに戻れても意味がない)。
		const url = new URL(page.url);
		url.searchParams.delete('error');
		// 今いる URL からクエリを1つ落とすだけで、route id を解決する話ではない。
		// eslint-disable-next-line svelte/no-navigation-without-resolve -- 現在の URL の書き換え (上のコメント参照)
		void goto(url, { replaceState: true, noScroll: true, keepFocus: true });
	});

	// メニュー項目は押せなくできないので、応答を待つ間の2回目を捨てる。
	let loggingOut = false;

	async function handleLogout() {
		if (loggingOut) return;
		loggingOut = true;
		try {
			await logout();
		} finally {
			loggingOut = false;
		}
	}

	async function logout() {
		try {
			const { error, response } = await client.POST('/api/v1/auth/logout');
			// サーバー側でセッションを破棄できていない状態のままログイン画面へ遷移すると、
			// 共有端末で「ログアウトしたつもり」で有効なセッションが残ってしまう。
			// 失敗時は遷移しない。
			if (!response.ok) {
				errorDialog.show(errorMessage(error), m.logout_error_title());
				return;
			}
		} catch {
			errorDialog.show(GENERIC_ERROR_MESSAGE(), m.logout_error_title());
			return;
		}
		// 認証が必要なarchive/folderの音声が、ログアウト後や共有端末で次の利用者に
		// 画面を渡した後も再生・表示され続けないよう停止する。
		nowPlaying.stop();
		// トップページは匿名でも開けるため、ログアウト後はログイン画面ではなくトップへ戻す。
		// 既にトップに居る場合、`goto` はURLが同じなので load を再実行しない
		// (ヘッダーがユーザー名のまま、要ログインのカードも並んだままになる)。
		// 遷移の有無に関わらず再取得させるため `invalidateAll()` を続けて呼ぶ。
		await goto(resolve('/'));
		await invalidateAll();
	}
</script>

<svelte:head><link rel="icon" href={favicon} /></svelte:head>

<!-- FOUC防止スクリプトはhooks.server.tsが%modewatcher.snippet%として差し込む
     (→ app.html)。ここでの自動注入は二重になるため無効化する。 -->
<ModeWatcher disableHeadScriptInjection />

<NavigationProgress />

<div class="flex min-h-svh flex-col">
	<header
		class="relative z-10 flex h-13 items-center justify-between gap-4 border-b bg-background px-4 md:h-14 md:px-6"
	>
		<!-- 押せる範囲 (::after) だけを 44px に広げる (→ docs/ui.md「UI 全般」)。 -->
		<a
			href={resolve('/')}
			class="relative shrink-0 text-lg leading-6 font-bold after:absolute after:inset-x-0 after:-inset-y-2.5"
			>WebLAV</a
		>
		<!-- 押せる範囲が横で重なるのを許し、間隔は 2px にする (→ docs/ui.md「UI 全般」)。 -->
		<div class="flex min-w-0 items-center gap-0.5">
			<ConnectionInfoDialog />
			<LanguageToggle />
			<ModeToggle />
			{#if page.data.user}
				<UserMenu username={page.data.user.username} onlogout={handleLogout} />
			{:else}
				<a href={resolve('/login')} class={headerTextActionClass}>{m.login_nav_label()}</a>
			{/if}
		</div>
	</header>
	<!-- 画面下部固定のプレイヤーがコンテンツの最後を隠さないよう、表示中だけ
	     バーの高さ分のpaddingを足す (幅や曲名で高さが変わるので測った値を使う)。 -->
	<div
		class="flex flex-1 flex-col"
		style:padding-bottom={nowPlaying.current !== null ? `${playerHeight}px` : undefined}
	>
		{@render children()}
	</div>
	<AudioPlayerBar bind:height={playerHeight} />
</div>

<FileViewer />

<ErrorDialog bind:open={errorDialog.open} message={errorDialog.message} title={errorDialog.title} />

<!-- ヘッダー (52px / md 以上 56px → docs/ui.md「UI 全般」) のすぐ下に出す。 -->
<Toaster position="top-center" offset={{ top: 64 }} mobileOffset={{ top: 60 }} />
