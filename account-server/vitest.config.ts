import { cloudflareTest, readD1Migrations } from '@cloudflare/vitest-pool-workers';
import { readFileSync } from 'node:fs';
import { defineConfig } from 'vitest/config';

/** テストでも手元と同じ秘密の値 (dev.vars.example) を使う。 */
function devVars(): Record<string, string> {
	return Object.fromEntries(
		readFileSync(new URL('./dev.vars.example', import.meta.url), 'utf8')
			.split('\n')
			.filter((line) => line && !line.startsWith('#'))
			.map((line) => [line.slice(0, line.indexOf('=')), line.slice(line.indexOf('=') + 1)])
	);
}

// `secrets.required` の値が無いという警告を出さないよう、process.env にも置く。
Object.assign(process.env, devVars());

export default defineConfig({
	plugins: [
		cloudflareTest(async () => ({
			wrangler: { configPath: './wrangler.jsonc' },
			miniflare: {
				bindings: {
					...devVars(),
					MAIL_LOG_ONLY: '1',
					// Stripe へはつながない (テストが fetch を差し替える)。webhook の署名はこの値で作る。
					STRIPE_SECRET_KEY: 'sk_test_dummy',
					STRIPE_WEBHOOK_SECRET: 'whsec_test',
					STRIPE_PERSONAL_MONTHLY_PRICE_ID: 'price_month',
					STRIPE_PERSONAL_YEARLY_PRICE_ID: 'price_year',
					STRIPE_ORG_PRICE_ID: 'price_org',
					GOOGLE_CLIENT_ID: 'google-client',
					GOOGLE_CLIENT_SECRET: 'google-secret',
					TEST_MIGRATIONS: await readD1Migrations('./migrations')
				}
			}
		}))
	],
	test: { setupFiles: ['./test/apply-migrations.ts'] }
});
