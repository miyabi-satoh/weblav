// 一覧のページ内の絞り込みの当たり方 (→ docs/ui.md「一覧の絞り込み」)。

/** 揃える。全角と半角・大文字と小文字を区別しない (ヘッダーの検索と同じ揃え方)。 */
function normalize(text: string): string {
	return text.normalize('NFKC').toLowerCase();
}

/** 前の字にまとめて揃える印。半角カナの濁点・半濁点 (ﾊﾞ) は、前の字と合わせて初めて「バ」になるため。 */
const VOICING_MARKS = new Set(['\uFF9E', '\uFF9F', '\u3099', '\u309A']);

/** 揃えた文字の並びと、それぞれが元の文字列の何文字目から何文字目 (コードポイント単位) から来たか。 */
function normalizedUnits(text: string): { chars: string[]; origins: number[][] } {
	const source = Array.from(text);
	const chars: string[] = [];
	const origins: number[][] = [];
	for (let index = 0; index < source.length; index++) {
		let cluster = source[index];
		const start = index;
		while (index + 1 < source.length && VOICING_MARKS.has(source[index + 1])) {
			cluster += source[++index];
		}
		const from = Array.from({ length: index - start + 1 }, (_, offset) => start + offset);
		for (const unit of Array.from(normalize(cluster))) {
			chars.push(unit);
			origins.push(from);
		}
	}
	return { chars, origins };
}

/** 空白 (全角を含む) で区切った語。揃えてから分ける。 */
export function parseFilterTerms(query: string): string[][] {
	return normalize(query)
		.split(/\s+/)
		.filter((term) => term !== '')
		.map((term) => Array.from(term));
}

/**
 * `terms` のどの語も `text` に当たれば、当たった文字の位置 (コードポイント単位) を返す。当たらなければ `null`。
 * 語は、続きのまま含まれていればそこに当て、無ければ、その順に間を空けて現れる文字に当てる (fzf と同じ考え方)。
 * 続きのままの当たりを先に見るのは、強調がばらけず、何で当たったか読み取りやすいため。
 */
export function matchFilter(text: string, terms: string[][]): number[] | null {
	const { chars, origins } = normalizedUnits(text);
	const hits = new Set<number>();
	for (const term of terms) {
		const positions = contiguousMatch(chars, term) ?? subsequenceMatch(chars, term);
		if (positions === null) return null;
		for (const position of positions) for (const origin of origins[position]) hits.add(origin);
	}
	return [...hits].sort((a, b) => a - b);
}

function contiguousMatch(chars: string[], term: string[]): number[] | null {
	for (let start = 0; start + term.length <= chars.length; start++) {
		if (term.every((char, offset) => chars[start + offset] === char)) {
			return term.map((_, offset) => start + offset);
		}
	}
	return null;
}

function subsequenceMatch(chars: string[], term: string[]): number[] | null {
	const positions: number[] = [];
	let next = 0;
	for (const char of term) {
		const found = chars.indexOf(char, next);
		if (found === -1) return null;
		positions.push(found);
		next = found + 1;
	}
	return positions;
}

/** 当たった位置で区切った断片。強調の表示に使う。 */
export function highlightSegments(
	text: string,
	hits: readonly number[]
): { text: string; hit: boolean }[] {
	const hitSet = new Set(hits);
	const segments: { text: string; hit: boolean }[] = [];
	Array.from(text).forEach((char, index) => {
		const hit = hitSet.has(index);
		const last = segments.at(-1);
		if (last && last.hit === hit) last.text += char;
		else segments.push({ text: char, hit });
	});
	return segments;
}
