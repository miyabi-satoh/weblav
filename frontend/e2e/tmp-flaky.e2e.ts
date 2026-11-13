import { expect, test } from '@playwright/test';

test('tmp flaky check (fails once)', () => {
	expect(test.info().retry).toBeGreaterThan(0);
});
