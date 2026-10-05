<script lang="ts">
	import * as m from '$lib/paraglide/messages.js';
	import { pageTitle } from '$lib/page-title';
</script>

<svelte:head><title>{pageTitle(m.help_eyebrow())}</title></svelte:head>

<p class="text-sm text-muted-foreground">{m.help_select_page()}</p>
