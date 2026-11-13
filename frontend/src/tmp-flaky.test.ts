import { expect, test } from 'vitest';

let attempts = 0;
test('tmp flaky check (fails once)', () => {
	attempts++;
	expect(attempts).toBeGreaterThan(1);
});
