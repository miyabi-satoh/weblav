import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { readVersion } from '../../scripts/cargo-version.mjs';
import { runPowerShell } from '../lib/powershell.mjs';

// 配る形の MSIX (`just msix` の dist/weblav-v<版>.msix) を WACK (Windows App Certification Kit) にかけ、必須のテストが
// すべて通ることを見る (docs/distribution.md「MSIX (Windows)」の「実機での確認」)。WACK は管理者の権限が要るので、
// 管理者でなければ UAC の確かめの窓を出して、管理者の PowerShell で流す。数分かかる

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..');
const APPCERT = path.join(
	process.env['ProgramFiles(x86)'],
	'Windows Kits',
	'10',
	'App Certification Kit',
	'appcert.exe'
);
const REPORT = path.join(repoRoot, 'target', 'e2e-msix', 'wack-report.xml');

/** 報告の XML のテストごとの結果 (名前・必須か・結果) */
function parseReport(xml) {
	return [...xml.matchAll(/<TEST\b([^>]*)>([\s\S]*?)<\/TEST>/g)].map(([, attrs, body]) => {
		const attr = (name) => new RegExp(`${name}="([^"]*)"`).exec(attrs)?.[1] ?? '';
		const result = /<RESULT>\s*(?:<!\[CDATA\[)?\s*(\w+)/.exec(body)?.[1] ?? '';
		return { name: attr('NAME'), optional: attr('OPTIONAL') === 'TRUE', result };
	});
}

test.describe('MSIX 版: WACK', () => {
	test('配る形の MSIX で、WACK の必須のテストがすべて通る', { timeout: 30 * 60_000 }, async () => {
		const msix = path.join(repoRoot, 'dist', `weblav-v${readVersion()}.msix`);
		await fs.access(msix);
		await fs.mkdir(path.dirname(REPORT), { recursive: true });
		await fs.rm(REPORT, { force: true });
		// appcert は相対パスを受け付けないので、どちらも絶対パスで渡す
		const script = `& '${APPCERT}' reset | Out-Null; & '${APPCERT}' test -appxpackagepath '${msix}' -reportoutputpath '${REPORT}' | Out-Null; exit $LASTEXITCODE`;
		const exitCode = await runPowerShell(
			`
$encoded = [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($args[0]))
$p = Start-Process -FilePath powershell -ArgumentList @('-NoProfile', '-EncodedCommand', $encoded) -Verb RunAs -WindowStyle Hidden -Wait -PassThru
[string]$p.ExitCode
`,
			[script]
		);
		const xml = await fs.readFile(REPORT, 'utf8').catch(() => {
			throw new Error(`WACK の報告がありません (終了コード ${exitCode.trim()})`);
		});
		const tests = parseReport(xml);
		assert.ok(tests.length > 0, '報告にテストがある');
		const failed = tests.filter((t) => !t.optional && t.result !== 'PASS');
		assert.deepEqual(failed, [], `必須のテストが通らない (報告: ${REPORT})`);
	});
});
