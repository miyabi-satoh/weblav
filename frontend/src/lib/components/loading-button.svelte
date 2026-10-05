<script lang="ts">
	import { Button, type ButtonProps } from '$lib/components/ui/button';
	import LoadingLabel from '$lib/components/loading-label.svelte';
	import { cn } from '$lib/utils';

	type Props = ButtonProps & {
		loading?: boolean;
	};

	let { loading = false, disabled, children, class: className, ...restProps }: Props = $props();
</script>

<Button disabled={loading || disabled} class={cn('relative', className)} {...restProps}>
	<LoadingLabel {loading}>
		{@render children?.()}
	</LoadingLabel>
</Button>
