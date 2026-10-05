// new-link-key.mjs と new-proof-key.mjs の共通の手順。鍵を作り、wrangler に secret として置き、公開鍵を src/pro.rs に足す形で出す。
import { spawnSync } from 'node:child_process';

/** 鍵の組を作り、wrangler に渡す secret の JSON と、公開鍵のバイト列を返す。 */
export async function newKeySecrets({ algorithm, usages, keyName, kidName, kid }) {
	const { privateKey, publicKey } = await crypto.subtle.generateKey(
		{ name: algorithm },
		true,
		usages
	);
	const secrets = JSON.stringify({
		[keyName]: JSON.stringify(await crypto.subtle.exportKey('jwk', privateKey)),
		[kidName]: kid
	});
	const raw = new Uint8Array(await crypto.subtle.exportKey('raw', publicKey));
	return { secrets, raw };
}

/** 公開鍵を src/pro.rs の配列の行 (`0x..` を `perRow` 個ずつ、`indent` で字下げ) にする。 */
export function byteRows(raw, perRow, indent) {
	const bytes = [...raw].map((b) => `0x${b.toString(16).padStart(2, '0')}`);
	const rows = [];
	for (let i = 0; i < bytes.length; i += perRow) {
		rows.push(`${indent}${bytes.slice(i, i + perRow).join(', ')},`);
	}
	return rows.join('\n');
}

/**
 * secret を標準入力で wrangler に渡し、`entry` を src/pro.rs の `list` に足すよう出す。
 * 公開鍵は wrangler の成否によらず出す。失敗しても secret が置かれていることがあり、控えが無いので後から出せない。
 */
export function putSecrets({ wranglerArgs, secrets, list, entry }) {
	const res = spawnSync('pnpm', ['exec', 'wrangler', ...wranglerArgs], {
		input: secrets,
		stdio: ['pipe', 'inherit', 'inherit']
	});
	if (res.status !== 0) {
		console.error(
			`\nwrangler が失敗した。secret は置かれたかもしれない (値は後から見られない)。置き直すか、次を src/pro.rs の ${list} (配る版) に足す:\n${entry}`
		);
		process.exit(res.status ?? 1);
	}
	console.log(`\nsrc/pro.rs の ${list} (配る版) に足す:\n${entry}`);
}
