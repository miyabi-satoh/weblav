import { isHttpError } from '@sveltejs/kit';
import { describe, expect, it, vi } from 'vitest';
import * as m from '$lib/paraglide/messages.js';
import { fetchOrError } from './load';

vi.mock('$app/paths', () => ({ resolve: (path: string) => path }));

function respond<T>(status: number, data?: T) {
	return Promise.resolve({ data, response: new Response(null, { status }) });
}

async function thrown(promise: Promise<unknown>): Promise<unknown> {
	try {
		await promise;
	} catch (e) {
		return e;
	}
	throw new Error('fetchOrError did not throw');
}

describe('fetchOrError', () => {
	it('returns the data on success', async () => {
		await expect(fetchOrError(respond(200, { id: 1 }), 'failed')).resolves.toEqual({ id: 1 });
	});

	it('shows the not-found message instead of the failed message on 404', async () => {
		const e = await thrown(fetchOrError(respond(404), 'failed'));
		expect(isHttpError(e, 404)).toBe(true);
		expect((e as { body: { message: string } }).body.message).toBe(m.error_not_found());
	});

	it('shows the failed message on other errors', async () => {
		const e = await thrown(fetchOrError(respond(500), 'failed'));
		expect(isHttpError(e, 500)).toBe(true);
		expect((e as { body: { message: string } }).body.message).toBe('failed');
	});
});
