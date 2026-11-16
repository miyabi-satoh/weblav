import { describe, expect, it, vi } from 'vitest';

const { get } = vi.hoisted(() => ({ get: vi.fn() }));
vi.mock('#lib/api/client.ts', () => ({ client: { GET: get } }));
vi.mock('$app/paths', () => ({ resolve: (path: string) => `/${path}` }));

const user = { username: 'alice' };

async function load() {
	const { load } = await import('./+layout');
	const event = { route: { id: '/' }, url: new URL('http://localhost/') };
	return (load as unknown as (event: unknown) => Promise<unknown>)(event);
}

describe('root layout load', () => {
	it('keeps the last known user when /auth/me cannot be reached, so the app shell stays', async () => {
		vi.resetModules();
		get.mockResolvedValueOnce({ data: user, response: new Response(null, { status: 200 }) });
		await load();

		get.mockRejectedValueOnce(new TypeError('Failed to fetch'));
		await expect(load()).resolves.toEqual({ user });

		get.mockResolvedValueOnce({ data: undefined, response: new Response(null, { status: 503 }) });
		await expect(load()).resolves.toEqual({ user });
	});

	it('fails when nothing is known yet (the first load)', async () => {
		vi.resetModules();
		get.mockRejectedValueOnce(new TypeError('Failed to fetch'));
		await expect(load()).rejects.toMatchObject({ status: 503 });
	});
});
