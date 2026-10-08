# 詳しい設定

## 設定ファイル (config.toml) を書く

設定の置き場所 (→ [運用する](09-maintenance.md)) に `config.toml` を作り、設定を書きます。
サイト設定の「サーバー」の欄を保存したことがあれば、もうあるので書き足します。同じ見出し (`[server]` など) は2つ書かないでください。
保存したら、WebLAV を起動し直します。

サイト設定を保存したときに「設定ファイル (config.toml) を読めません。」と出たら、書いた内容に誤り (引用符の閉じ忘れ、同じ見出しを2つ書いた、など) があります。`config.toml` を直してから、もう一度保存します。

- **Windows**: メモ帳で書き、設定の置き場所に、ファイルの種類を「すべてのファイル」にして `config.toml` の名前で保存する
- **macOS**: ターミナルで次のとおり打つと、テキストエディットで開く

  ```
  cd ~/Library/Containers/com.amiiby.weblav/Data/Library/Application\ Support/com.amiiby.weblav && touch config.toml && open -e config.toml
  ```

- **Ubuntu**: ターミナルで次のとおり打つと、テキストエディターで開く

  ```
  mkdir -p ~/.config/weblav && touch ~/.config/weblav/config.toml && xdg-open ~/.config/weblav/config.toml
  ```

## 設定の一覧

書かなかった項目は既定値で動きます。

| 見出し | 項目 | 既定値 | 意味 |
|---|---|---|---|
| `[server]` | `bind` | `"0.0.0.0"` | 待ち受けるアドレス。`"127.0.0.1"` にすると、WebLAV を動かしているパソコンからしかつながらない |
| | `port` | `3000` | 待ち受けるポート |
| `[log]` | `filter` | `"info"` | ログの細かさ。`"debug"` にすると詳しくなる |
| | `output` | `"file"` | ログの出し先。`"file"` はファイル、`"stdout"` は画面 (ターミナルから動かすとき) |
| `[session]` | `secret` | `""` | ログインの署名に使う鍵。空なら自動で作る。ふつうは書かない |
| | `secure_cookie` | `false` | HTTPS で使うときに `true` にする (→ 下の「HTTPS で使う」) |
| | `expiry_days` | `14` | ログインを保つ日数 |
| `[upload]` | `max_size_mb` | `100` | アップロードできるファイルの大きさの上限 (MB) |

- `port`・`expiry_days`・`max_size_mb` は、管理画面のサイト設定でも変えられます。
- `bind` にパソコンの特定のアドレス (`192.168.1.10` など) を書くと、「セットアップ」の画面を開けません。最初の管理者を作る間だけ、既定値に戻します。
- 「詳しいログを出す」を画面でオンにしても、起動し直すと `filter` の値に戻ります。
- `secret` を書き間違えると起動できなくなり、書き換えると全員のログインが切れます。

## HTTPS で使う

nginx や Caddy などのリバースプロキシを WebLAV と同じパソコンに置き、HTTPS で受けて WebLAV へ渡します。

1. `config.toml` に次のとおり書く

   ```
   [server]
   bind = "127.0.0.1"

   [session]
   secure_cookie = true
   ```

2. プロキシが `X-Forwarded-For` を付けて渡すようにする。nginx では次の2行を足す (Caddy は不要)

   ```
   proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
   proxy_set_header Host $host;
   ```

**2 を忘れると、ネットワーク上の誰でも最初の管理者を作れてしまいます。**

最初の管理者の作成と「公開できるフォルダ」は、WebLAV を動かしているパソコンの Edge か Chrome で、メニューから開いて操作します。
