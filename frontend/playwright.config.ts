import { defineConfig, devices } from '@playwright/test';
import { ADMIN_STORAGE_STATE } from './e2e/auth';

// e2eはSvelteKitのSPAビルド単体ではなく、backend(/api/v1)込みのweblavに対して行う。
// backendの起動方法は複数ある(release exe・`just dev-backend`・CIでの一時起動など)ため
// ここでは決め打てず、webServerでの自動起動はしない。呼び出し側が起動済みのURLを渡す。
const baseURL = process.env.E2E_BASE_URL;
if (!baseURL) {
	throw new Error(
		'E2E_BASE_URL が未設定です。backendを起動してからそのURLを設定して実行する: ' +
			'E2E_BASE_URL=http://127.0.0.1:3000 E2E_ADMIN_PASSWORD=... pnpm run test:e2e'
	);
}

export default defineConfig({
	// cookie が無ければ表示言語がブラウザの言語で決まり (→ docs/ui.md「UI 全般」)、日時の書式も
	// それに従うため、実行する PC の言語で結果が変わらないよう固定する。
	use: { baseURL, locale: 'en-US' },
	// 管理者ログインをテストごとにUI操作からやり直さない
	// (公式ベストプラクティス、認証状態の使い回し: https://playwright.dev/docs/auth)。
	// setupプロジェクトが一度だけログインしてstorageStateを保存し、e2eプロジェクトはそれを使い回す
	// (未ログイン状態が要るテストはファイル側でstorageStateを空に上書きする、→ anonymous.e2e.ts)。
	projects: [
		{ name: 'setup', testMatch: '**/*.setup.ts' },
		{
			name: 'e2e',
			testMatch: '**/*.e2e.{ts,js}',
			use: { ...devices['Desktop Chrome'], storageState: ADMIN_STORAGE_STATE },
			dependencies: ['setup']
		}
	]
});
