//! 422 の内訳を、画面が文言を組み立てられる形で返すための型 (→ docs/architecture.md「エラーの契約と表示言語」)。
//!
//! バックエンドの `message` は英語のデバッグ用で、画面には出さない。表示する文言は
//! `frontend/messages/*.json` が持ち、ここで渡す値 (タイトル・軸名・公開範囲) を
//! 差し込んで組み立てる。サーバー側に翻訳を持たせないのは、表示言語を知っているのが
//! ブラウザー側だけであり、同じ語を2箇所で管理したくないため。
//!
//! **全ての 422 に付けるわけではない**。汎用の「入力内容を確認してください」で用が
//! 足りるものはそのままにし、値を見せないと直しようがないものだけを足す。

use serde::Serialize;
use utoipa::ToSchema;

/// 422 の内訳。`kind` で場合分けし、文言に差し込む値を併せて渡す。
#[derive(Debug, Serialize, ToSchema)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ValidationDetail {
    /// 「作成者のみ」のグループに置けるのは、グループの作成者の「作成者のみ」のコンテンツだけ
    /// (→ docs/access.md「作成者のみのグループ」)。
    PrivateGroupNeedsOwnPrivate,
    /// 直下に、グループの作成者の「作成者のみ」でない子があるグループは「作成者のみ」にできない
    /// (→ docs/access.md「作成者のみのグループ」)。
    GroupChildrenNotOwnPrivate,
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
    /// 同じ名前の「共有」が既にある (→ docs/folders.md「共有」)。
    #[serde(rename_all = "camelCase")]
    RootNameTaken { name: String },
    /// そのフォルダーは「共有」に登録済み (→ docs/folders.md「共有」)。
    #[serde(rename_all = "camelCase")]
    RootAlreadyRegistered { name: String },
    /// 共有の書き込みの許可を取り直すとき、窓で別のフォルダーを選んだ。`name` は取り直そうとした共有の名前。
    #[serde(rename_all = "camelCase")]
    RepickedOtherFolder { name: String },
    /// WebLAV 自身の置き場 (設定とデータ) の中は、共有にできない (→ docs/folders.md「共有」)。
    RootInsideOwnDirs,
    /// 受け取ったファイルが WebLAV のバックアップではない・壊れている (→ docs/access.md「バックアップとリストア」)。
    BackupInvalid,
    /// 受け取ったバックアップが、今より新しい版で作られている (→ docs/access.md「バックアップとリストア」)。
    #[serde(rename_all = "camelCase")]
    BackupTooNew { app_version: String },
    /// 再スキャンで、アーカイブの登録先のフォルダーが見つからない (→ docs/archive.md「スキャン」)。
    ArchiveFolderMissing,
    /// 再スキャンで、読めない場所があった (→ docs/archive.md「スキャン」)。
    /// `path` は最初に読めなかった場所の、登録先からの相対パス。登録先そのものなら空文字。
    #[serde(rename_all = "camelCase")]
    ArchiveFolderUnreadable { path: String },
    /// 共有の中に作るファイル・フォルダーの名前に使えない (空・`/` を含む・ドットで始まる・長すぎる・
    /// Windows で使えない名前など。→ `api::shares::paths::validate_name`)。
    EntryNameInvalid,
    /// 置き先に同じ名前のファイル・フォルダーが既にある。
    #[serde(rename_all = "camelCase")]
    EntryNameTaken { name: String },
    /// フォルダーを、そのフォルダー自身やその中へ移動・コピーしようとした。
    TransferIntoItself,
    /// 共有そのもの・中に別の共有や WebLAV のデータを含むものを、名前の変更・移動・削除・上書きしようとした。
    /// 共有の登録や WebLAV 自身が、その場所を指したまま見失うため。
    EntryProtected,
}
