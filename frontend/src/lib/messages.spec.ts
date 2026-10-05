import { describe, expect, it } from 'vitest';
import ja from '../../messages/ja.json';

/** 単位として数の後に来る語。プレースホルダーは名前で数かどうかを決めず、後ろの語で見る。 */
const UNIT = '(?:秒|分|時間|日|週|か月|年|件|回|文字|行|個|つ|ページ|[KMGT]B)';
/** 数字の後に日本語か単位、またはプレースホルダーの後に単位が、普通の空白を挟んで続く所。 */
const BREAKABLE_NUMBER_UNIT = new RegExp(
	`\\d (?=[一-龠々ぁ-んァ-ヶ]|${UNIT})|\\{\\w+\\} (?=${UNIT})`
);

describe('ja messages', () => {
	// 数と単位の間で折り返さないよう、ノーブレークスペースにする (→ docs/ui.md「UI 全般」)。
	it('has no breakable space between a number and its unit', () => {
		const breakable = Object.entries(ja)
			.filter(([key, value]) => key !== '$schema' && BREAKABLE_NUMBER_UNIT.test(String(value)))
			.map(([key]) => key);
		expect(breakable).toEqual([]);
	});
});
