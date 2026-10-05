<script lang="ts">
	import { page } from '$app/state';
	import { resolve } from '$app/paths';
	import { afterNavigate } from '$app/navigation';
	import ChevronDownIcon from '@lucide/svelte/icons/chevron-down';
	import { Button } from '$lib/components/ui/button';
	import * as m from '$lib/paraglide/messages.js';
	import { listItemClass } from '$lib/list-row';
	import {
		docListClass,
		docPageEyebrowClass,
		docPageHeadingClass,
		docPageLeadClass,
		helpContentClass,
		helpPageClass,
		helpSidebarClass
	} from '$lib/page-layout';
	import type { LayoutProps } from './$types';

	let { data, children }: LayoutProps = $props();

	// スマートフォン幅のページでは目次をボタンの中にたたみ、本文から見せる (→ docs/help.md)。
	let tocOpen = $state(false);
	afterNavigate(() => (tocOpen = false));
	const tocCollapsed = $derived(page.params.slug !== undefined && !tocOpen);
</script>

<div class={helpPageClass}>
	<div class={helpSidebarClass}>
		<p class={docPageEyebrowClass}>{m.help_eyebrow()}</p>
		{#if page.params.slug}
			<Button
				variant="outline"
				class="mt-3 w-full justify-between md:hidden"
				aria-expanded={tocOpen}
				aria-controls="help-toc"
				onclick={() => (tocOpen = !tocOpen)}
			>
				{m.help_toc_button()}
				<ChevronDownIcon class={['transition-transform', tocOpen && 'rotate-180']} />
			</Button>
		{/if}
		<div id="help-toc" class={[tocCollapsed && 'max-md:hidden']}>
			<!-- 個別ページでは本文の先頭の題がそのページの h1 なので、こちらは見出しにしない。 -->
			<svelte:element this={page.params.slug ? 'p' : 'h1'} class={docPageHeadingClass}
				>{m.help_title()}</svelte:element
			>
			<p class={docPageLeadClass}>{m.help_lead()}</p>

			<!-- 目次の見出しは Markdown 由来なので、訳が無いページは原典 (日本語) の見出しが
		     並ぶ。ページ全体の `<html lang>` と食い違うため、リンクごとに実際の言語を付ける。 -->
			<nav class={['mt-8', docListClass]}>
				{#each data.pages as helpPage (helpPage.slug)}
					<a
						href={resolve('/help/[slug]', { slug: helpPage.slug })}
						lang={helpPage.locale}
						class={[
							listItemClass,
							'flex items-center justify-between px-4 py-3 text-sm hover:bg-muted',
							page.params.slug === helpPage.slug && 'bg-muted'
						]}
					>
						{helpPage.title}
					</a>
				{/each}
			</nav>

			<!-- 同梱している第三者のソフトウェアの表示 (→ docs/third-party-licenses.md)。
		     マニュアルの1ページではないので目次の中には入れず、その下に置く。 -->
			<p class="mt-4">
				<a
					href={resolve('/licenses')}
					class="relative text-xs text-muted-foreground underline underline-offset-4 after:absolute after:inset-x-0 after:-inset-y-3.5"
				>
					{m.licenses_link()}
				</a>
			</p>
		</div>
	</div>

	<div class={helpContentClass}>
		{@render children()}
	</div>
</div>
