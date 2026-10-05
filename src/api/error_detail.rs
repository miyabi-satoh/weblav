//! 422 (と Free の上限の 409) の内訳を、画面が文言を組み立てられる形で返すための型 (→ docs/architecture.md「エラーの契約と表示言語」)。
//!
//! バックエンドの `message` は英語のデバッグ用で、画面には出さない。表示する文言は
//! `frontend/messages/*.json` が持ち、ここで渡す値 (タイトル・軸名・公開範囲) を
//! 差し込んで組み立てる。サーバー側に翻訳を持たせないのは、表示言語を知っているのが
//! ブラウザ側だけであり、同じ語を2箇所で管理したくないため。
//!
//! **全ての 422 に付けるわけではない**。汎用の「入力内容を確認してください」で用が
//! 足りるものはそのままにし、値を見せないと直しようがないものだけを足す。

use serde::Serialize;
use utoipa::ToSchema;

/// 422 の内訳。`kind` で場合分けし、文言に差し込む値を併せて渡す。
#[derive(Debug, Serialize, ToSchema)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ValidationDetail {
    /// グループは「本人のみ」にできない (→ docs/access.md「公開範囲」)。
    GroupCannotBePrivate,
    /// 軸名にプレースホルダーの記号 (`{` `}`) を含んでいる。
    AxisNameHasBraces,
    /// 同じ名前の軸が既にある。
    #[serde(rename_all = "camelCase")]
    AxisNameTaken { name: String },
    /// 予約プレースホルダーと同じ名前は軸に使えない (→ docs/archive.md「表示タイトル」)。
    #[serde(rename_all = "camelCase")]
    AxisNameReserved { name: String },
    /// 表示タイトルのテンプレートが、存在しない軸を参照している。
    #[serde(rename_all = "camelCase")]
    TemplateUnknownAxis { name: String },
    /// 外部の AI の答えを取り込むとき、この軸の定義が決まりに合わない (階層の番号・値の空や重複など)。
    /// どの決まりかまでは分けない。答えを直すのは AI に頼み直すことになり、軸が分かれば足りるため。
    #[serde(rename_all = "camelCase")]
    ImportAxisInvalid { name: String },
    /// 表示タイトルのテンプレートで使われている軸は削除できない。
    #[serde(rename_all = "camelCase")]
    AxisInUseByTemplate { name: String },
    /// 同じ名前のユーザーが既にある。大文字小文字だけの違いも同じ名前とみなす。
    #[serde(rename_all = "camelCase")]
    UsernameTaken { name: String },
    /// 最後の管理者は降格・削除できない (→ docs/access.md「admin の最後の1人」)。
    LastAdmin,
    /// 同じ名前の「公開できるフォルダ」が既にある (→ docs/folders.md「公開できるフォルダ」)。
    #[serde(rename_all = "camelCase")]
    RootNameTaken { name: String },
    /// そのフォルダは「公開できるフォルダ」に登録済み (→ docs/folders.md「公開できるフォルダ」)。
    #[serde(rename_all = "camelCase")]
    RootAlreadyRegistered { name: String },
    /// WebLAV 自身の置き場 (設定とデータ) の中は、公開できるフォルダにできない (→ docs/folders.md「公開できるフォルダ」)。
    RootInsideOwnDirs,
    /// 受け取ったファイルが WebLAV のバックアップではない・壊れている (→ docs/access.md「バックアップとリストア」)。
    BackupInvalid,
    /// 受け取ったバックアップが、今より新しい版で作られている (→ docs/access.md「バックアップとリストア」)。
    #[serde(rename_all = "camelCase")]
    BackupTooNew { app_version: String },
    /// Free の上限に当たった (→ docs/pro.md「上限を数えて止める」)。409 に付ける。
    #[serde(rename_all = "camelCase")]
    FreeLimit {
        target: super::free_limit::FreeLimitTarget,
        limit: i64,
    },
}
