import { describe, expect, it } from 'vitest';
import { readNdjson } from './ndjson';

function streamOf(chunks: string[]): ReadableStream<Uint8Array> {
	const encoder = new TextEncoder();
	return new ReadableStream({
		start(controller) {
			for (const chunk of chunks) controller.enqueue(encoder.encode(chunk));
			controller.close();
		}
	});
}

describe('readNdjson', () => {
	it('読みの切れ目をまたぐ行も、そろってから1件ずつ渡す', async () => {
		const items: unknown[] = [];
		await readNdjson(streamOf(['{"a":1}\n{"a"', ':2}\n\n{"a":3}']), (item) => items.push(item));
		expect(items).toEqual([{ a: 1 }, { a: 2 }, { a: 3 }]);
	});

	it('読みの切れ目で割れた多バイト文字も読む', async () => {
		const bytes = new TextEncoder().encode('{"t":"日本"}\n');
		const items: unknown[] = [];
		const stream = new ReadableStream<Uint8Array>({
			start(controller) {
				controller.enqueue(bytes.slice(0, 8));
				controller.enqueue(bytes.slice(8));
				controller.close();
			}
		});
		await readNdjson(stream, (item) => items.push(item));
		expect(items).toEqual([{ t: '日本' }]);
	});
});
