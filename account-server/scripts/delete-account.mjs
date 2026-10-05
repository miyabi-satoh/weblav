// 窓口のアカウントを消す (→ src/account-deletion.ts)。削除の請求を問い合わせで受けたときに、運営者が流す。
// 先にサブスクを Stripe でその場で解約する (→ docs/pro.md「アカウントと販売の窓口」の「消す」)。
// 購入の台帳とサブスクの行は、結び付きだけを外して残る。
//
// 使い方: STRIPE_SECRET_KEY=<その環境の秘密鍵> node scripts/delete-account.mjs <メールアドレス> [--env staging]
import { spawnSync } from 'node:child_process';
import { DELETE_ACCOUNT_STATEMENTS } from '../src/account-deletion.ts';

const [rawEmail, ...rest] = process.argv.slice(2);
const email = (rawEmail ?? '').trim().toLowerCase();
const env = rest[0] === '--env' && rest[1] ? ['--env', rest[1]] : [];
const stripeKey = process.env.STRIPE_SECRET_KEY;
// SQL に書き込むので、アドレスの形でないものは通さない (引用符を含む値で文を壊さないように)。
if (!/^[^\s'"@;]+@[^\s'"@;]+$/.test(email) || (rest.length > 0 && env.length === 0) || !stripeKey) {
	console.error(
		'usage: STRIPE_SECRET_KEY=<key> node scripts/delete-account.mjs <email> [--env staging]'
	);
	process.exit(1);
}

function d1(sql, json = false) {
	const result = spawnSync(
		'pnpm',
		[
			'exec',
			'wrangler',
			'd1',
			'execute',
			'DB',
			'--remote',
			...env,
			...(json ? ['--json'] : []),
			'--command',
			sql
		],
		{ stdio: ['inherit', json ? 'pipe' : 'inherit', 'inherit'], encoding: 'utf8' }
	);
	if (result.status !== 0) process.exit(result.status ?? 1);
	return json ? JSON.parse(result.stdout) : undefined;
}

async function stripe(method, path, query) {
	const res = await fetch(`https://api.stripe.com/v1/${path}${query ? `?${query}` : ''}`, {
		method,
		headers: { authorization: `Bearer ${stripeKey}` }
	});
	// 見つからない (404) か、もう解約した (DELETE への 400) サブスクは、終わっているので先へ進む (アプリの cancelSubscription と同じ)。
	const done = res.status === 404 || (method === 'DELETE' && res.status === 400);
	if (!res.ok && !done) throw new Error(`Stripe ${res.status}: ${await res.text()}`);
	return res.json();
}

// D1 の行にあるサブスクと、まだ行の無い組織向けのサブスク (metadata の account_email で探す)。
const rows = d1(
	`SELECT id FROM subscriptions
	 WHERE account_id = (SELECT id FROM accounts WHERE email = '${email}') AND id LIKE 'sub_%'`,
	true
);
const ids = new Set(rows[0].results.map((r) => r.id));
const found = await stripe(
	'GET',
	'subscriptions/search',
	new URLSearchParams({ query: `metadata['account_email']:'${email}' AND -status:'canceled'` })
);
for (const sub of found.data ?? []) ids.add(sub.id);
for (const id of ids) {
	await stripe('DELETE', `subscriptions/${encodeURIComponent(id)}`);
	console.log(`canceled ${id}`);
}

d1(DELETE_ACCOUNT_STATEMENTS.map((s) => `${s.replaceAll('?1', `'${email}'`)};`).join('\n'));
