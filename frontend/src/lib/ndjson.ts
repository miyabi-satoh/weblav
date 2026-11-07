/**
 * 1行に1つの JSON (NDJSON) の流れを、行が届くたびに `onItem` へ渡す。
 * 1行が読みの切れ目をまたいで届いても、行がそろってから渡す。
 */
export async function readNdjson<T>(
	stream: ReadableStream<Uint8Array>,
	onItem: (item: T) => void
): Promise<void> {
	const reader = stream.pipeThrough(new TextDecoderStream()).getReader();
	let pending = '';
	for (;;) {
		const { done, value } = await reader.read();
		if (done) break;
		pending += value;
		const lines = pending.split('\n');
		pending = lines.pop() ?? '';
		for (const line of lines) {
			if (line.trim() !== '') onItem(JSON.parse(line) as T);
		}
	}
	if (pending.trim() !== '') onItem(JSON.parse(pending) as T);
}
