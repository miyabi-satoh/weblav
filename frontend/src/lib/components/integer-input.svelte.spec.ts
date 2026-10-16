import { describe, expect, it } from 'vitest';
import { page, userEvent } from 'vitest/browser';
import { render } from 'vitest-browser-svelte';
import IntegerInput from './integer-input.svelte';

function input(): HTMLInputElement {
	return page.getByRole('textbox').element() as HTMLInputElement;
}

describe('IntegerInput', () => {
	it('updates the value and stays valid for a number in range', async () => {
		const props = $state({ id: 'n', value: 5, min: 1, max: 10 });
		await render(IntegerInput, props);
		await userEvent.fill(page.getByRole('textbox'), '8');

		expect(props.value).toBe(8);
		expect(input().validity.valid).toBe(true);
	});

	it('keeps the typed text and blocks submission for a number out of range', async () => {
		const props = $state({ id: 'n', value: 5, min: 1, max: 10 });
		await render(IntegerInput, props);
		await userEvent.fill(page.getByRole('textbox'), '11');

		expect(props.value).toBe(5);
		expect(input().value).toBe('11');
		expect(input().validity.customError).toBe(true);
	});

	it('shows the new value when it changes from outside after invalid text', async () => {
		const props = $state({ id: 'n', value: 5, min: 1, max: 10 });
		await render(IntegerInput, props);
		await userEvent.fill(page.getByRole('textbox'), 'abc');
		props.value = 3;

		await expect.poll(() => input().value).toBe('3');
		expect(input().validity.valid).toBe(true);
	});

	it('rewrites a readable number in the usual form when leaving the field', async () => {
		const props = $state({ id: 'n', value: 5, min: 1, max: 10 });
		await render(IntegerInput, props);
		await userEvent.fill(page.getByRole('textbox'), '０７');
		await userEvent.tab();

		expect(props.value).toBe(7);
		expect(input().value).toBe('7');
	});

	it('ties the unit to the field for screen readers', async () => {
		await render(IntegerInput, { id: 'n', value: 5, min: 1, max: 10, unit: 'MB' });

		await expect.element(page.getByRole('textbox')).toHaveAccessibleDescription('MB');
	});
});
