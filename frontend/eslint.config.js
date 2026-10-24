import prettier from 'eslint-config-prettier';
import path from 'node:path';
import js from '@eslint/js';
import svelte from 'eslint-plugin-svelte';
import { defineConfig, includeIgnoreFile } from 'eslint/config';
import globals from 'globals';
import ts from 'typescript-eslint';

const gitignorePath = path.resolve(import.meta.dirname, '.gitignore');

export default defineConfig(
	includeIgnoreFile(gitignorePath),
	{
		// shadcn-svelte が生成するコンポーネント (registry からの取得物なので対象外)
		ignores: ['src/lib/components/ui/**']
	},
	{
		// openapi-typescript が生成する型定義 (pnpm run generate:api-types で再生成されるので対象外)
		ignores: ['src/lib/api/schema.d.ts']
	},
	js.configs.recommended,
	ts.configs.recommended,
	svelte.configs.recommended,
	prettier,
	svelte.configs.prettier,
	{
		languageOptions: { globals: { ...globals.browser, ...globals.node } },
		rules: {
			// typescript-eslint strongly recommend that you do not use the no-undef lint rule on TypeScript projects.
			// see: https://typescript-eslint.io/troubleshooting/faqs/eslint/#i-get-errors-from-the-no-undef-rule-about-global-variables-not-being-defined-even-though-there-are-no-typescript-errors
			'no-undef': 'off'
		}
	},
	{
		files: ['**/*.svelte', '**/*.svelte.ts', '**/*.svelte.js'],
		languageOptions: {
			parserOptions: {
				projectService: true,
				extraFileExtensions: ['.svelte'],
				parser: ts.parser
			}
		}
	},
	{
		rules: {
			// invalidateAll と invalidate は取り直すときに page.state を空に戻すので、そこに置いた一覧の絞り込みの語が消える
			// (→ $lib/list-filter.svelte.ts)。page.state を残す refreshAll を使う。
			'no-restricted-imports': [
				'error',
				{
					paths: [
						{
							name: '$app/navigation',
							importNames: ['invalidateAll', 'invalidate'],
							message: 'refreshAll を使う (page.state を残すため)。'
						}
					]
				}
			]
		}
	}
);
