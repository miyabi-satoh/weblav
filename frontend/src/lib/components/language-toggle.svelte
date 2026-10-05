<script lang="ts">
	import GlobeIcon from '@lucide/svelte/icons/globe';
	import { getLocale, setLocale, locales, isLocale } from '$lib/paraglide/runtime';
	import * as m from '$lib/paraglide/messages.js';
	import * as DropdownMenu from '$lib/components/ui/dropdown-menu';
	import { headerIconActionClass } from '$lib/header-action';

	const localeLabels: Record<string, () => string> = {
		en: m.language_en,
		ja: m.language_ja
	};
</script>

<DropdownMenu.Root>
	<DropdownMenu.Trigger class={headerIconActionClass}>
		<GlobeIcon class="size-5" />
		<span class="sr-only">{m.language_toggle_label()}</span>
	</DropdownMenu.Trigger>
	<DropdownMenu.Content align="end">
		<DropdownMenu.RadioGroup
			bind:value={
				getLocale,
				(locale) => {
					if (isLocale(locale)) setLocale(locale);
				}
			}
		>
			{#each locales as locale (locale)}
				<DropdownMenu.RadioItem value={locale}>
					{localeLabels[locale]?.() ?? locale}
				</DropdownMenu.RadioItem>
			{/each}
		</DropdownMenu.RadioGroup>
	</DropdownMenu.Content>
</DropdownMenu.Root>
