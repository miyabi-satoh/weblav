-- ファイルアップロード型コンテンツ(type='file')用のカラムを追加する。
--
-- blob_hash はアップロードされた実体のSHA-256ハッシュ(64桁16進文字列)。実ファイルは
-- `AppDirs::blobs_dir()` 配下に `<blob_hash>` という名前で保存する(content-addressed
-- storage)。同じ内容のファイルを複数のcontents行から参照できる(重複排除)。
-- file_name はダウンロード時に使う元のファイル名(パス区切りを含まないbasenameのみ)。
-- file_size はバイト数(アップロード時にサーバー側で計測した値。クライアントの申告値は信用しない)。
--
-- link=url必須/folder=path必須と同様、「fileならこの3カラムが必須」はアプリケーション層
-- (Rust)で検証する。blob_hashに同じ値を持つ行が0件になったら実ファイルを削除する
-- (参照カウントの代わりにその都度COUNTで確認する。GC専用の別テーブルは今回不要)。
ALTER TABLE contents ADD COLUMN blob_hash TEXT;
ALTER TABLE contents ADD COLUMN file_name TEXT;
ALTER TABLE contents ADD COLUMN file_size INTEGER;

-- 削除・差し替え時に「同じblob_hashを参照する行が他に残っているか」を数える
-- クエリで使うため。
CREATE INDEX idx_contents_blob_hash ON contents (blob_hash);
