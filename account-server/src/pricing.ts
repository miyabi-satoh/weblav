/**
 * 値付けの値 (→ docs/account-server.md「値付けの値」)。原価の割合が分かるので、リポジトリに書かず秘密の値で置く。
 * 1つでも無いか読めなければ投げる。窓口のどの入口も、DB に書く前にここで止める (src/index.ts)。
 */

/** 原価を出すための値。 */
export type Rates = {
	/** 1 ドルの円。 */
	usdJpy: number;
	/** Gemini の単価 (100万トークンあたりの USD)。出力は思考のトークンを含む。 */
	inputUsdPerMtok: number;
	outputUsdPerMtok: number;
	/** 検索の単価 (1リクエストあたりの USD)。 */
	searchUsdPerRequest: number;
};

export type Pricing = {
	/** 1回の購入で付ける原価 (milli_yen)。値段は Stripe の Price で決まる。 */
	purchaseGrant: number;
	/** 新しいアカウントに一度だけ付ける無料の分 (milli_yen)。 */
	freeGrant: number;
	rates: Rates;
};

export function pricing(env: Env): Pricing {
	const read = (name: string, value: string | undefined, integer: boolean) => {
		const n = Number(value);
		if (!value || !Number.isFinite(n) || n <= 0 || (integer && !Number.isInteger(n))) {
			throw new Error(`${name} is missing or invalid`);
		}
		return n;
	};
	return {
		purchaseGrant: read('PURCHASE_GRANT_MILLI_YEN', env.PURCHASE_GRANT_MILLI_YEN, true),
		freeGrant: read('FREE_GRANT_MILLI_YEN', env.FREE_GRANT_MILLI_YEN, true),
		rates: {
			usdJpy: read('USD_JPY', env.USD_JPY, false),
			inputUsdPerMtok: read('INPUT_USD_PER_MTOK', env.INPUT_USD_PER_MTOK, false),
			outputUsdPerMtok: read('OUTPUT_USD_PER_MTOK', env.OUTPUT_USD_PER_MTOK, false),
			searchUsdPerRequest: read('SEARCH_USD_PER_REQUEST', env.SEARCH_USD_PER_REQUEST, false)
		}
	};
}

/**
 * 使った原価を、1回の購入で付く量を 100万としたときの量に直す。WebLAV へ返し、誰がどれだけ使ったかの記録に使う。
 * 原価の円のままでは返さない (原価の割合が分かるため)。
 */
export function usedShare(env: Env, cost: number): number {
	return Math.ceil((cost * 1_000_000) / pricing(env).purchaseGrant);
}
