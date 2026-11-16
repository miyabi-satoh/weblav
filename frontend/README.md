# frontend

WebLAV の画面。SvelteKit (Svelte 5) を静的な SPA として `build/` に書き出し、Rust のサーバーがそれをバイナリに埋め込んで配る。画面の言語は日本語と英語 (Paraglide)。

## 要るもの

- Node.js 26 以降 (`package.json` の `engines`)
- pnpm 12 (`package.json` の `packageManager`)
- 画面を API とつないで動かすときは、リポジトリの直下の Rust のサーバー (→ `../README.md`)

## 動かす・確かめる

コマンドはリポジトリの直下で `just` から流す (定義は `../justfile`)。

| コマンド              | すること                                                                          |
| --------------------- | --------------------------------------------------------------------------------- |
| `just install`        | 依存・git の hook・e2e 用の Chromium を入れる                                     |
| `just dev`            | 開発用のサーバーと画面をまとめて起動する (`/api` はサーバーへ渡す)                |
| `just frontend-build` | `build/` に書き出す                                                               |
| `just lint`           | eslint と表記の検査 (Tailwind の任意値・長音・括弧) を流す。Rust の clippy も流す |
| `just check`          | svelte-check で型を検査する                                                       |
| `just test`           | vitest を流す。Rust のテストも流す                                                |
| `just e2e-local`      | 使い捨てのサーバーに対して Playwright の e2e を流す                               |
| `just api-types`      | サーバーの API から `src/lib/api/schema.d.ts` を作り直す                          |

## 置き場所

- `src/routes/`: 画面
- `src/lib/`: 部品と共通の処理 (`components/ui/` は shadcn-svelte の部品)
- `messages/`: 画面の文言 (`ja.json`・`en.json`)
- `e2e/`: Playwright の e2e
- `scripts/`: e2e の起動、マニュアルの画像の撮り直し (`just manual-shots`) など

書き方の決まりは `../AGENTS.md` の「コードの規約」、画面の決めごとは `../docs/ui.md` にある。
