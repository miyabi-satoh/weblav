# 窓口（weblav.amiiby.com）

キーを持たない利用者も AI の機能を使えるよう、運営者のキーで AI と検索に中継し、使った分を前払いのクレジット（コードと以下では残高と呼ぶ）から引くサーバー（窓口）。アカウント・PC の登録・購入・中継を受け持つ。

## 作り

- `account-server/` に置く。Cloudflare Workers・Hono・D1・Resend・Stripe で作り、Stripe・Google・Apple・AI・検索の API は `fetch` で呼ぶ。
- 窓口の画面は `/account/`、WebLAV と Stripe が呼ぶ入口は `/v1/` に置き、それ以外は紹介と規約類の静的なページを返す。
- 売るのは使えば減る残高で、WebLAV は使うたびに窓口へ問い合わせるので、オフラインで確かめる証明は持たない。

この文書とコードの語は、画面では次のように呼ぶ。利用者に仕組みの語を見せないため。

| この文書 | 画面 |
| --- | --- |
| WebLAV とアカウントを結ぶ・外す | PC を登録・登録を解除（英語は link・unlink） |
| 結んでいる WebLAV | 登録している PC |
| 窓口 | WebLAV のアカウント |

## サインイン

- メールのリンク・Google・Apple でサインインする。
- 外部のサインインで受け取ったメールが確かめ済みのときだけ、同じメールのアカウントに結ぶ。
- Google は、Google が現在の持ち主を確かめられる Gmail または Google Workspace のメールだけを受け付ける。
- Apple の戻りは Apple のサイトからの POST（`form_post`）なので、往復の印の Cookie は `SameSite=None` にする。
- Apple の転送用アドレスを使う人には、Stripe の Checkout へメールを渡さず、支払いの画面で入れてもらう。
- ブラウザのサインインの状態は、30 日の `__Host-` Cookie と D1 の `sessions` で持つ。

## PC をアカウントに登録する

管理者が別の端末のブラウザから WebLAV を操作することがあるので、ブラウザを WebLAV が動く PC の `127.0.0.1` へ戻さず、窓口で認めた印を WebLAV が問い合わせる。

1. WebLAV は 32 バイトの乱数の16進である検証用の値を作り、その SHA-256 の16進を `challenge` として、サイト名 `name`・PC 名 `pc` とともに `/account/link` の URL に付ける。
2. 管理者はその URL を開いてサインインし、PC の登録を認める。窓口の画面と WebLAV の画面は、どちらも `challenge` の頭の6桁を確認コードとして出す。他人が始めた申し込みを認めないよう、見比べてもらうため。
3. 窓口は `link_approvals` に `challenge`・`account_id`・`name`・`pc_name`・`expires_at`・`created_at` を残す。認めてからトークンへ替えるまでの期限は10分である。
4. WebLAV は検証用の値を `code_verifier` として `POST /v1/links/token` に送り、まだ認められていなければ `{ "status": "pending" }` と 202 を受けて間を置いて問い合わせ直す。
5. 認められていれば窓口は、トークンを作るのと対応する行を消すのを1つのまとまりで流し、32 バイトの乱数のトークンを返す。窓口はトークンのハッシュだけを `app_tokens` に持つので、URL を見た人はトークンに替えられない。

- アプリ用のトークンは期限を持たず、WebLAV が `DELETE /v1/token` を呼ぶか、窓口のアカウントの画面で登録を外すと失効する。
- アプリ用の入口は `Authorization: Bearer <トークン>` で呼ぶ。トークンが無いか失効していれば 401 を返す。
- 同じアカウントに登録する PC の数には上限を置かない。

## 中継

- `POST /v1/ai` で WebLAV が組み立てた `system`・`user`・任意の `attachments` を受け、Gemini の `gemini-3.5-flash-lite` へ送って返事をそのまま返す。
- モデル・出力の上限（16,000 トークン）・入力の上限（`system` と `user` の合計 200,000 文字）は窓口で固定し、WebLAV から変えさせない。
- 添付は base64 の PDF・PNG・JPEG・WebP を1つだけ受け、合計 14,000,000 文字までにする。元のファイルでは約 10 MB であり、ほかの文書は WebLAV が文字を取り出して送る。
- 送った文と返事は窓口のログや D1 に残さない。
- 呼ぶ前に、残りが入力の原価の見積もり（文は1文字を1トークン、添付は 50 KB を PDF の1ページと数える）に届くかを確かめ、届かなければ 402 を返す。返事が届いてから使った原価を引く。出力の分は残りを超えても通し、返事が届かなかった回は引かない。
- 同じアカウントの呼び出しは、同時に3件まで重ねて受ける（`ai_in_flight`）。登録した PC とその利用者が残高を共有するため。残りを超えて使われる分は、重なった数 × 1回の出力の原価までに収まる。4件目と、`AI_LIMITER`（アカウントごとに 10 秒で 20 回）の上限は 429 を返す。
- Gemini に届かなかったとき（時間切れ・接続の失敗）は、Gemini の失敗の返事と同じ 502 `upstream` を返す。
- 本文が添付と文の上限を足した大きさを超えるときは、読まずに 413 を返す。
- 成功時と、AI が使った量を返した失敗時は `x-weblav-used` ヘッダーで使った量を返す。値は1回の購入で付く量を 1,000,000 とした整数であり、WebLAV は誰がどれだけ使ったかをこの値で記録する。
- 窓口には WebLAV のユーザー名を送らない。

## 検索の中継

- `POST /v1/search` で、AI が集めるリンクの候補を Perplexity の Search API から受けて WebLAV へ返す。
- `queries` は空でない語を 1〜5 個、各 200 文字までにし、任意で国（ISO 3166-1 alpha-2）・言語（ISO 639-1）・新しさの絞り込み `year` を受ける。
- 返す候補は最大 20 件で、各件の題・URL・任意の日付・抜粋を含む。
- 成功した回だけ1リクエスト分の原価を引き、`x-weblav-used` ヘッダーで使った量を返す。検索の失敗では引かない。残りの確かめと重ねて受ける数は、中継と同じ。
- 検索も中継と同じアプリ用トークンと `AI_LIMITER` を使い、窓口には WebLAV のユーザー名を送らない。

## 残高

- 原価の単位は 1/1000 円（milli_yen）の整数である。Gemini の原価は入力・出力・思考のトークン数、単価、円への換算から切り上げて出し、検索は1リクエストの原価を使う。
- 残高は付与ごとに1行の `grants` で持つ。購入の付与は1回の購入ごとに同じ量を付け、無料の付与は新しいアカウントに一度だけ付ける。
- 引くときは購入の付与を古い順に使い、無料の付与を最後に使う。
- 使った分は、引いた付与ごとに `consumptions` に1行ずつ残す。購入の付与から引いた分の売値は `purchases.amount × consumptions.milli_yen ÷ consumptions.grant_size` で出す。
- `consumptions.grant_size` には引いた時点の `grants.granted` を写すので、アカウントを消して付与の行が無くなった後も売値に直せる。
- `GET /v1/balance` はメールアドレスと残りの割合を `email`・`remaining_percent` で返す。WebLAV には金額や回数ではなく割合だけを見せる。
- その月に付けた無料の付与の合計が上限に達すると、新しいアカウントへの無料の付与を止める。付けられなかったアカウントには、翌月以降に残高の問い合わせか中継をしたときに付け直す。

## 値付けの値

- 購入と無料の付与の量・無料の付与の月の上限・Gemini と検索の単価・円の換算は、売値と並べると原価の割合が分かるので、リポジトリに書かず秘密の値に置く。
- 値の名前は `PURCHASE_GRANT_MILLI_YEN`・`FREE_GRANT_MILLI_YEN`・`FREE_MONTHLY_CAP_YEN`・`USD_JPY`・`INPUT_USD_PER_MTOK`・`OUTPUT_USD_PER_MTOK`・`SEARCH_USD_PER_REQUEST` である。
- 1つでも無いか読めなければ、窓口のどの入口も D1 に書く前に 500 で止める。誤った値で付けたり引いたりせず、サインインのリンクを使った後の途中で止まらないようにする。

## 購入

- Stripe の Checkout で残高を売り、`POST /v1/stripe/webhook` で受けた支払いの知らせから付与する。支払いの画面から戻った先では付与しない。
- この製品の目印は `weblav-ai`、Price の ID は `STRIPE_AI_CREDITS_PRICE_ID` で読む。
- 日本からの買い手には直接、それ以外の買い手には Managed Payments で売る。
- 知らせは id を `stripe_events` に記録し、終えた知らせを送り直されても処理し直さない。付与の前には Checkout Session を Stripe から取り直して中身を確かめる。
- 購入は `purchases` に残し、返金または不審請求の申し立てが届いたら、その購入の付与の残りだけを取り消す。使った分は取り戻さない。

## アカウントを消す

- 問い合わせを受けて、運営者が `account-server/scripts/delete-account.mjs` で消す。
- `grants`・`app_tokens`・`sessions`・`identities` はアカウントと一緒に消す。
- `purchases` と `consumptions` はアカウントとの結び付きを外して7年残す。台帳に残すのは取引の ID・製品・額・通貨・日時・取り消したか・Managed Payments の取引か・国内の取引かである。
- `purchases.card_country` と `purchases.buyer_country` はアカウントを消すと消す。
- 使われないアカウントは自動では消さない。
