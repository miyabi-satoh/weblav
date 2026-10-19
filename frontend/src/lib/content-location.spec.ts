import { describe, expect, it } from 'vitest';
import { resolveRelativePath } from './content-location';

describe('resolveRelativePath', () => {
	it('ファイルのあるフォルダーから解決する', () => {
		expect(resolveRelativePath('docs/spec/01-login.md', 'assets/a.png')).toBe(
			'docs/spec/assets/a.png'
		);
		expect(resolveRelativePath('docs/spec/01-login.md', './02-home.md')).toBe(
			'docs/spec/02-home.md'
		);
		expect(resolveRelativePath('README.md', 'docs/a.md')).toBe('docs/a.md');
	});

	it('`..` で上のフォルダーへ上がる。登録先より上は解決しない', () => {
		expect(resolveRelativePath('docs/spec/01-login.md', '../ui.md')).toBe('docs/ui.md');
		expect(resolveRelativePath('docs/a.md', '../../b.md')).toBeUndefined();
	});

	it('クエリと見出しへの飛び先を外し、エスケープを戻す', () => {
		expect(resolveRelativePath('a/b.md', 'c%20d.md#見出し')).toBe('a/c d.md');
		expect(resolveRelativePath('a/b.md', 'c.png?raw=1')).toBe('a/c.png');
	});

	it('相対パスでないものは解決しない', () => {
		for (const href of [
			'https://example.com/a.png',
			'mailto:a@example.com',
			'/docs/a.md',
			'#見出し',
			'%E0'
		]) {
			expect(resolveRelativePath('a/b.md', href)).toBeUndefined();
		}
	});
});
