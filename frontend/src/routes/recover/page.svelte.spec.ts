import { afterEach, describe, expect, it, vi } from 'vitest';
import { page, userEvent } from 'vitest/browser';
import { render } from 'vitest-browser-svelte';
import type { beforeNavigate as BeforeNavigateFn } from '$app/navigation';
import RecoverPage from './+page.svelte';

type BeforeNavigate = Parameters<Parameters<typeof BeforeNavigateFn>[0]>[0];

const { beforeNavigate, goto, refreshAll, post } = vi.hoisted(() => ({
	beforeNavigate: vi.fn(),
	goto: vi.fn(),
	refreshAll: vi.fn(),
	post: vi.fn()
}));
vi.mock('$app/navigation', () => ({ beforeNavigate, goto, refreshAll }));
vi.mock('$app/state', () => ({ page: { data: { user: null } } }));
vi.mock('#lib/api/client.ts', () => ({ client: { POST: post } }));

/** ラベルには必須の印が付くので、欄は id で取る。 */
function field(id: string) {
	return page.elementLocator(document.getElementById(id) as HTMLElement);
}

/** 戻し終えて、新しいリカバリコードを見せたところまで進める。 */
async function recover() {
	post.mockResolvedValue({
		data: {
			user: { id: 1, username: 'alice', role: 'user', hasRecoveryCode: true },
			recoveryCode: 'AAAAA-BBBBB-CCCCC-DDDDD'
		},
		response: new Response(null, { status: 200 })
	});
	await render(RecoverPage);
	for (const [id, value] of [
		['username', 'alice'],
		['recovery-code', 'ZZZZZ-ZZZZZ-ZZZZZ-ZZZZZ'],
		['new-password', 'new-password'],
		['new-password-confirm', 'new-password']
	]) {
		await userEvent.fill(field(id), value);
	}
	await page.getByRole('button', { name: 'パスワードを再設定する' }).click();
	await expect.element(page.getByText('AAAAA-BBBBB-CCCCC-DDDDD')).toBeVisible();
}

function navigate(to: URL) {
	const guard: (navigation: BeforeNavigate) => void = beforeNavigate.mock.calls[0][0];
	const cancel = vi.fn();
	guard({ type: 'link', to: { url: to }, cancel } as unknown as BeforeNavigate);
	return cancel;
}

describe('recover page', () => {
	afterEach(() => {
		beforeNavigate.mockReset();
		goto.mockReset();
		post.mockReset();
	});

	it('asks before leaving while the new code has not been kept', async () => {
		await recover();
		const to = new URL('http://localhost/');
		const cancel = navigate(to);

		expect(cancel).toHaveBeenCalled();
		await expect.element(page.getByText('リカバリコードを保管せずに移動しますか？')).toBeVisible();
		await page.getByRole('button', { name: '移動する' }).click();
		await expect.poll(() => goto.mock.calls.length).toBe(1);
		expect(goto).toHaveBeenCalledWith(to);
	});

	it('does not ask before the code is shown', async () => {
		await render(RecoverPage);
		const cancel = navigate(new URL('http://localhost/login'));

		expect(cancel).not.toHaveBeenCalled();
	});
});
