import type { Handle } from '@sveltejs/kit/hooks';
import { createInitialModeExpression } from 'mode-watcher';
import { getTextDirection } from '#lib/paraglide/runtime.js';
import { paraglideMiddleware } from '#lib/paraglide/server.js';

const handleParaglide: Handle = ({ event, resolve }) =>
	// 表示言語は cookie で決める (→ vite.config.ts の strategy) ので、paraglide は要求の URL を書き換えない。
	// 書き換えた要求で解決し直すのは URL の strategy のときだけなので、元の要求のまま解決する。
	paraglideMiddleware(event.request, ({ locale }) =>
		resolve(event, {
			transformPageChunk: ({ html }) =>
				html
					.replace('%paraglide.lang%', locale)
					.replace('%paraglide.dir%', getTextDirection(locale)) // mode-watcher公式のFOUC防止スクリプト。<ModeWatcher disableHeadScriptInjection />
					// と対にして、二重に注入されないようにしている (→ +layout.svelte)。
					.replace('%modewatcher.snippet%', `<script>${createInitialModeExpression()}</script>`)
		})
	);

export const handle: Handle = handleParaglide;
