<script lang="ts">
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import { pageTitle } from '$lib/page-title';
	import { docPageClass, docPageHeadingClass, docPageLeadClass } from '$lib/page-layout';
	import * as m from '$lib/paraglide/messages.js';

	// 読み込みの失敗 (fetchOrError) と、どのページにも当たらない URL がここに来る。
	// 共通ヘッダーの下に、何が起きたかと戻り先を出す。
	let heading = $derived(
		page.status === 404
			? m.app_error_not_found_title()
			: page.status === 403
				? m.app_error_forbidden_title()
				: m.app_error_title()
	);
	// どのページにも当たらない URL は、SvelteKit が英語の "Not Found" を載せてくるので使わない。
	let lead = $derived(
		page.status === 404 ? m.app_error_not_found_lead() : (page.error?.message ?? '')
	);
</script>

<svelte:head><title>{pageTitle(heading)}</title></svelte:head>

<div class={docPageClass}>
	<h1 class={docPageHeadingClass}>{heading}</h1>
	<p class={docPageLeadClass}>{lead}</p>
	<a
		href={resolve('/')}
		class="mt-4 inline-flex h-11 items-center text-sm text-primary underline underline-offset-4"
		>{m.app_error_home_link()}</a
	>
</div>
