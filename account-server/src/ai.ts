/**
 * 作者のキーで Gemini へ中継する (→ docs/account-server.md「中継」)。
 * モデル・思考の強さ・出力の上限・入力の上限はここで決め打ちし、WebLAV からは変えさせない (生の API の代わりにさせないため。Google APIs Terms 4.a)。
 * 送った文も結果もログに残さない。
 */

import type { Rates } from './pricing';

/** Interactions API (ai.google.dev/api/interactions-api)。 */
const GEMINI_INTERACTIONS_URL = 'https://generativelanguage.googleapis.com/v1/interactions';

/** 中継するモデル。軸の案・リンクの選別・1往復の質問に足り、単価が低い。 */
const MODEL = 'gemini-3.5-flash-lite';

/**
 * 用途ごとの思考の強さ。無い用途は、モデルの既定 (minimal) のまま送る。
 * 簡易チャットは、計算や推理の問いに minimal では間違った答えを言い切るので上げる。
 * low は考えるかどうかがモデル任せで間違いが残り、high は答えが変わらないまま待ち時間が延びる。
 */
const THINKING_LEVELS = { chat: 'medium' } as const;
/** WebLAV が伝える用途。 */
export type Task = keyof typeof THINKING_LEVELS;

/** 返事の長さの上限。軸の案や要約に足りる長さ。 */
const MAX_OUTPUT_TOKENS = 16_000;
/**
 * 受ける文の長さ (指示文と文の合計、UTF-16 の単位)。軸の定義で送るフォルダとファイルの名前の一覧と、
 * 簡易チャットで送るテキストや文書から取り出した文字に足りる長さ。1回の原価の上限を決める値でもある。
 */
const MAX_INPUT_CHARS = 200_000;
/** 添付の数。簡易チャットは開いているファイル1つを送る。 */
const MAX_ATTACHMENTS = 1;
/**
 * 添付の大きさ (base64 にした文字数) の合計。元のファイルで約 10 MB。
 * Gemini が受ける上限 (PDF は 50 MB) より小さくし、1回の原価と Worker のメモリを抑える。
 */
const MAX_ATTACHMENT_CHARS = 14_000_000;
/** 添付できる種類。Gemini が中身を読めるもの。ほかの文書は、WebLAV が文字を取り出して文として送る。 */
const ATTACHMENT_TYPES: Record<string, 'document' | 'image'> = {
	'application/pdf': 'document',
	'image/png': 'image',
	'image/jpeg': 'image',
	'image/webp': 'image'
};
/** 返事を待つ上限。WebLAV の待つ時間 (60 秒) より短くし、WebLAV が先に諦めて、使った分だけ引かれることを避ける。 */
const TIMEOUT_MS = 50_000;

/** 添付するファイル。`data` は base64。 */
export type Attachment = { mime_type: string; data: string };

/** WebLAV が組み立てて送る中身。 */
export type Prompt = { system?: string; user: string; attachments?: Attachment[]; task?: Task };

/** 受けた本文を `Prompt` として読む。形が違うときは `undefined`、長すぎるか大きすぎるときは `'too_long'`。 */
export function parsePrompt(body: unknown): Prompt | 'too_long' | undefined {
	if (typeof body !== 'object' || body === null) return undefined;
	const { system, user, attachments, task } = body as Record<string, unknown>;
	if (typeof user !== 'string' || (system !== undefined && typeof system !== 'string')) {
		return undefined;
	}
	if (task !== undefined && (typeof task !== 'string' || !Object.hasOwn(THINKING_LEVELS, task))) {
		return undefined;
	}
	const files: Attachment[] = [];
	if (attachments !== undefined) {
		if (!Array.isArray(attachments)) return undefined;
		for (const item of attachments) {
			if (typeof item !== 'object' || item === null) return undefined;
			const { mime_type: type, data } = item as Record<string, unknown>;
			if (typeof type !== 'string' || typeof data !== 'string') return undefined;
			if (
				!Object.hasOwn(ATTACHMENT_TYPES, type) ||
				data.length % 4 !== 0 ||
				!/^[A-Za-z0-9+/]+={0,2}$/.test(data)
			) {
				return undefined;
			}
			files.push({ mime_type: type, data });
		}
	}
	if (user.length + (system?.length ?? 0) > MAX_INPUT_CHARS) return 'too_long';
	if (files.length > MAX_ATTACHMENTS) return 'too_long';
	if (files.reduce((sum, file) => sum + file.data.length, 0) > MAX_ATTACHMENT_CHARS) {
		return 'too_long';
	}
	return {
		...(system === undefined ? {} : { system }),
		user,
		...(files.length === 0 ? {} : { attachments: files }),
		...(task === undefined ? {} : { task: task as Task })
	};
}

/** Gemini の返事。`body` は WebLAV へそのまま返す。`cost` は使った原価 (milli_yen)。 */
export type Relayed =
	| { ok: true; body: string; cost: number }
	| { ok: false; status: number; body: string; cost: number };

/**
 * 返事の `usage` から原価 (milli_yen、切り上げ) を出す。
 * `usage` が無いか読めなければ `undefined` (返事の形が崩れている)。
 */
export function costOf(usage: unknown, rates: Rates): number | undefined {
	if (typeof usage !== 'object' || usage === null) return undefined;
	const u = usage as Record<string, unknown>;
	const valid = (key: string) =>
		typeof u[key] === 'number' && Number.isFinite(u[key]) && (u[key] as number) >= 0;
	// 入力と出力の数が読めなければ、使った量が分からない。思考のトークンは、使わなければ無いことがある。
	if (!valid('total_input_tokens') || !valid('total_output_tokens')) return undefined;
	const count = (key: string) => (valid(key) ? (u[key] as number) : 0);
	const usd =
		(count('total_input_tokens') * rates.inputUsdPerMtok +
			(count('total_output_tokens') + count('total_thought_tokens')) * rates.outputUsdPerMtok) /
		1_000_000;
	return Math.ceil(usd * rates.usdJpy * 1000);
}

/**
 * 添付1つのトークン数の見積もり。Gemini 3 の既定の解像度で、画像は1枚 1,120、PDF は1ページ 560 に
 * ページの文字の分が足される (https://ai.google.dev/gemini-api/docs/media-resolution 、2026-10-17 に確かめた)。
 * PDF の文字の分は読まないと分からないので、1ページを高い解像度と同じ 1,120 と見る。
 */
const IMAGE_TOKENS = 1_120;
const PDF_PAGE_TOKENS = 1_120;
/** ページの数を読めない PDF (ページの情報を圧縮して持つもの) は、50 KB を1ページと見る。 */
const PDF_BYTES_PER_PAGE_FALLBACK = 50_000;

/**
 * PDF のページの数。ページのオブジェクト (`/Type /Page`。ページの束の `/Pages` は除く) を数える。
 * 圧縮した中にしか無ければ数えられないので `undefined`。見積もりにだけ使うので、厳密でなくてよい。
 */
export function pdfPageCount(base64: string): number | undefined {
	const bytes = atob(base64);
	const pages = bytes.match(/\/Type\s*\/Page(?![a-zA-Z])/g)?.length ?? 0;
	return pages > 0 ? pages : undefined;
}

function attachmentEstimate(file: Attachment): number {
	if (ATTACHMENT_TYPES[file.mime_type] === 'image') return IMAGE_TOKENS;
	const pages =
		pdfPageCount(file.data) ?? Math.ceil((file.data.length * 3) / 4 / PDF_BYTES_PER_PAGE_FALLBACK);
	return pages * PDF_PAGE_TOKENS;
}

/**
 * 入力のトークン数の見積もり。文は1文字を1トークン、添付は `attachmentEstimate`。
 * 送る前に残りが足りるかを見るのと、Gemini が使った量を返さなかったときに引く額に使う。
 */
export function inputEstimate(prompt: Prompt): number {
	return (
		prompt.user.length +
		(prompt.system?.length ?? 0) +
		(prompt.attachments ?? []).reduce((sum, file) => sum + attachmentEstimate(file), 0)
	);
}

/** 入力だけの原価の見積もり (milli_yen)。残りがこれに届かなければ、送らずに断る。 */
export function inputCostEstimate(prompt: Prompt, rates: Rates): number {
	return costOf({ total_input_tokens: inputEstimate(prompt), total_output_tokens: 0 }, rates) ?? 0;
}

/** 受ける本文の大きさ (バイト)。添付の上限に、文と JSON の分を足した大きさ。これを超える本文は読まずに断る。 */
export const MAX_BODY_BYTES = MAX_ATTACHMENT_CHARS + 4 * MAX_INPUT_CHARS + 10_000;

/** Gemini へ送る。届かなかったときは投げる (引かない)。 */
export async function relay(env: Env, prompt: Prompt, rates: Rates): Promise<Relayed> {
	const request: Record<string, unknown> = {
		model: MODEL,
		// 添付があるときは、利用者の入力の1つのまとまりに、添付と文を並べる (v1 の形。v1beta とは違う)。
		input:
			prompt.attachments === undefined
				? prompt.user
				: [
						{
							type: 'user_input',
							content: [
								...prompt.attachments.map((file) => ({
									type: ATTACHMENT_TYPES[file.mime_type],
									data: file.data,
									mime_type: file.mime_type
								})),
								{ type: 'text', text: prompt.user }
							]
						}
					],
		// やり取りを Google 側に保存させない (止めても、不正利用の監視のための保持は規約どおり残る)。
		store: false,
		generation_config: {
			max_output_tokens: MAX_OUTPUT_TOKENS,
			...(prompt.task === undefined ? {} : { thinking_level: THINKING_LEVELS[prompt.task] })
		}
	};
	if (prompt.system !== undefined) request.system_instruction = prompt.system;
	const res = await fetch(GEMINI_INTERACTIONS_URL, {
		method: 'POST',
		headers: { 'x-goog-api-key': env.GEMINI_API_KEY ?? '', 'content-type': 'application/json' },
		body: JSON.stringify(request),
		signal: AbortSignal.timeout(TIMEOUT_MS)
	});
	const body = await res.text();
	let usage: unknown;
	try {
		usage = (JSON.parse(body) as { usage?: unknown }).usage;
	} catch {
		usage = undefined;
	}
	let cost = costOf(usage, rates);
	// 成功したのに使った量が分からなければ、無料にせず、入力の全部と出力の上限を使ったものとして引く。
	if (cost === undefined && res.ok) {
		console.error('Gemini responded without usage');
		cost = costOf(
			{ total_input_tokens: inputEstimate(prompt), total_output_tokens: MAX_OUTPUT_TOKENS },
			rates
		);
	}
	cost ??= 0;
	if (res.ok) return { ok: true, body, cost };
	// 中身を含みうるので本文は残さず、状態だけを残す。
	console.error('Gemini responded', res.status);
	return { ok: false, status: res.status, body, cost };
}
