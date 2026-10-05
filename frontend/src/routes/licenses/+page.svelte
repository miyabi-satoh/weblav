<script lang="ts">
	import ChevronRightIcon from '@lucide/svelte/icons/chevron-right';
	import { SvelteSet } from 'svelte/reactivity';
	import { listItemClass } from '$lib/list-row';
	import { pageTitle } from '$lib/page-title';
	import {
		docListClass,
		docPageClass,
		docPageEyebrowClass,
		docPageHeadingClass,
		docPageLeadClass
	} from '$lib/page-layout';
	import * as m from '$lib/paraglide/messages.js';
	import type { LicenseList, LicensePackage } from './+page';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();

	let sections = $derived([
		{ key: 'rust', title: m.licenses_section_app(), list: data.rust },
		{ key: 'npm', title: m.licenses_section_frontend(), list: data.npm }
	]);

	/**
	 * 開いている行。本文は数百件あり長いので、開いた行の分だけ描く。
	 * Rust と npm で同じ名前のパッケージがありうるので、節の名前を添えて区別する。
	 */
	const opened = new SvelteSet<string>();

	function onToggle(event: Event, key: string) {
		if ((event.currentTarget as HTMLDetailsElement).open) opened.add(key);
		else opened.delete(key);
	}

	function textsOf(list: LicenseList, pkg: LicensePackage) {
		return pkg.texts.map((index) => list.texts[index]);
	}
</script>

<svelte:head><title>{pageTitle(m.licenses_title())}</title></svelte:head>

<div class={docPageClass}>
	<p class={docPageEyebrowClass}>{m.licenses_eyebrow()}</p>
	<h1 class={docPageHeadingClass}>{m.licenses_title()}</h1>
	<p class={docPageLeadClass}>{m.licenses_lead()}</p>

	{#each sections as section (section.key)}
		<section class="mt-10">
			<h2 class="text-lg font-bold">{section.title}</h2>
			{#if section.list === null}
				<p class="mt-3 text-sm text-muted-foreground">{m.licenses_unavailable()}</p>
			{:else}
				{@const list = section.list}
				<ul class={['mt-3', docListClass]}>
					{#each list.packages as pkg (pkg.name)}
						{@const key = `${section.key}:${pkg.name}`}
						<li class={listItemClass}>
							<details class="group" ontoggle={(event) => onToggle(event, key)}>
								<!-- 開閉の印は自前で置く。summary を flex にすると既定の三角が消えるため。 -->
								<summary
									class="flex min-h-11 items-center gap-2 px-4 py-3 text-sm select-none hover:bg-muted/50"
								>
									<ChevronRightIcon
										class="size-4 shrink-0 transition-transform group-open:rotate-90"
										strokeWidth={1.5}
									/>
									<!-- スマートフォン幅では、式をいつも次の行に左寄せで置く。入りきらない行だけ回すと、
									     行ごとに式の位置が変わって揃わない。広い幅では同じ行の右に寄せる。 -->
									<span class="flex min-w-0 flex-1 flex-wrap items-baseline gap-x-2 gap-y-0.5">
										<!-- 下線の後ろでも折れるようにする。ハイフンと違い下線では折れないので、
										     windows_x86_64_msvc のような名前が狭い幅で枠を越える。 -->
										<span class="font-bold"
											>{#each pkg.name.split(/(?<=_)/) as part, i (i)}{#if i > 0}<wbr
													/>{/if}{part}{/each}</span
										>
										<span class="text-muted-foreground">{pkg.versions.join(', ')}</span>
										<!-- 式は語の区切りでだけ折る。`Apache-2.0` がハイフンで折れると読めない。 -->
										<span
											class="basis-full text-muted-foreground sm:ml-auto sm:basis-auto sm:text-right"
											>{#each pkg.license.split(' ') as term, i (i)}{i > 0 ? ' ' : ''}<span
													class="whitespace-nowrap">{term}</span
												>{/each}</span
										>
									</span>
								</summary>
								{#if opened.has(key)}
									<div class="space-y-3 px-4 pb-4">
										{#if pkg.repository !== null}
											<!-- ソースの置き場所を出す。MPL-2.0 の依存が入っており、受け取る人に
											     ソースの入手先を知らせる必要があるため (→ docs/third-party-licenses.md)。
											     リンクは開閉の summary の外に置く。中に置くと、押したときに開閉とリンクの
											     どちらが働くかが紛らわしく、支援技術にも扱いにくい。 -->
											<p class="flex flex-wrap items-center gap-x-1 text-sm">
												{m.licenses_source()}:
												<a
													href={pkg.repository}
													target="_blank"
													rel="external noopener noreferrer"
													class="inline-flex min-h-11 items-center wrap-anywhere text-primary underline underline-offset-4"
												>
													{pkg.repository}
												</a>
											</p>
										{/if}
										{#each textsOf(list, pkg) as text, index (index)}
											<div>
												<h3 class="text-xs font-bold text-muted-foreground">{text.name}</h3>
												<!-- URL・メールアドレス・罫線など、空白の無い長い並びが狭い幅で枠を越えるので、どこでも折り返す。 -->
												<p
													class="mt-1 rounded-md bg-muted px-3 py-2 text-xs leading-relaxed wrap-anywhere whitespace-pre-wrap"
												>
													{text.text}
												</p>
											</div>
										{/each}
									</div>
								{/if}
							</details>
						</li>
					{/each}
				</ul>
			{/if}
		</section>
	{/each}
</div>
