import type { Handle } from '@sveltejs/kit';
import { createInitialModeExpression } from 'mode-watcher';
import { getTextDirection } from '$lib/paraglide/runtime';
import { paraglideMiddleware } from '$lib/paraglide/server';

const handleParaglide: Handle = ({ event, resolve }) =>
	paraglideMiddleware(event.request, ({ request, locale }) => {
		event.request = request;

		return resolve(event, {
			transformPageChunk: ({ html }) =>
				html
					.replace('%paraglide.lang%', locale)
					.replace('%paraglide.dir%', getTextDirection(locale))
					// mode-watcher公式のFOUC防止スクリプト。<ModeWatcher disableHeadScriptInjection />
					// と対にして、二重に注入されないようにしている (→ +layout.svelte)。
					.replace('%modewatcher.snippet%', `<script>${createInitialModeExpression()}</script>`)
		});
	});

export const handle: Handle = handleParaglide;
