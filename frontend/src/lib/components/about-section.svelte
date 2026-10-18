<script lang="ts">
	import { resolve } from '$app/paths';
	import * as m from '$lib/paraglide/messages.js';
	import { CONTACT_URL, PRIVACY_URL, TERMS_URL } from '$lib/site-links';

	let { version, build }: { version: string; build: string | null } = $props();

	// ビルド番号は bp-carnet と同じく版の後ろに括弧で添える (→ docs/distribution.md「版の番号」)。
	let versionText = $derived(build ? `${version} (${build})` : version);

	const linkClass = 'inline-flex min-h-11 items-center text-sm underline underline-offset-4';
</script>

<div class="flex flex-col">
	<p class="text-sm">
		WebLAV <span class="text-muted-foreground"
			>{m.admin_settings_about_version({ version: versionText })}</span
		>
	</p>
	<ul class="flex flex-wrap gap-x-5">
		<li>
			<a href={TERMS_URL} target="_blank" rel="external noopener noreferrer" class={linkClass}
				>{m.admin_settings_about_terms()}</a
			>
		</li>
		<li>
			<a href={PRIVACY_URL} target="_blank" rel="external noopener noreferrer" class={linkClass}
				>{m.admin_settings_about_privacy()}</a
			>
		</li>
		<li>
			<a href={CONTACT_URL} target="_blank" rel="external noopener noreferrer" class={linkClass}
				>{m.admin_settings_about_contact()}</a
			>
		</li>
	</ul>
	<ul class="flex flex-wrap gap-x-5">
		<li><a href={resolve('/help')} class={linkClass}>{m.help_eyebrow()}</a></li>
		<li><a href={resolve('/licenses')} class={linkClass}>{m.licenses_link()}</a></li>
	</ul>
</div>
