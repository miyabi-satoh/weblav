/**
 * 1行に1つの JSON (NDJSON) の流れを、行が届くたびに `onItem` へ渡す。
 * 1行が読みの切れ目をまたいで届いても、行がそろってから渡す。
 */
export async function readNdjson<T>(
	stream: ReadableStream<Uint8Array>,
	onItem: (item: T) => void
): Promise<void> {
	const reader = stream.getReader();
	const decoder = new TextDecoder();
	let pending = '';
	for (;;) {
		const { done, value } = await reader.read();
		if (done) break;
		// `stream: true`: 読みの切れ目で割れた多バイト文字を、次の読みとつなげて読む。
		pending += decoder.decode(value, { stream: true });
		const lines = pending.split('\n');
		pending = lines.pop() ?? '';
		for (const line of lines) {
			if (line.trim() !== '') onItem(JSON.parse(line) as T);
		}
	}
	pending += decoder.decode();
	if (pending.trim() !== '') onItem(JSON.parse(pending) as T);
}
