-- 「公開できるフォルダ」に名前と、削除済みの印を持たせる (→ docs/design.md 8.2)。
--
-- 名前: コンテンツの登録側の画面では、起点のフルパスの代わりにこの名前を出す。
-- 名前はフルパスの代わりに見分ける手掛かりなので、同じ名前は許さない
-- (大文字小文字だけの違いも同じとみなす。`users.username` と同じく NOCASE)。
--
-- 削除済みの印: 登録を削除しても行は残す。残ったコンテンツの場所を、起点を隠したまま
-- 「(存在しない公開フォルダ) / その先」と出すために、どこまでが起点だったかが要るため。
-- 配信・ピッカー・一覧では削除済みを外す。同じパスを登録し直したら印を消して戻す
-- (`path` の UNIQUE はそのまま)。名前の重なりは、削除済みを除いて見る。
--
-- 既存の行はフォルダ名 (パスの最後の要素) にする。区切りは `/` と `\` の両方を見る
-- (Windows の行も同じ DB 形式で持つため)。ドライブのルートのように最後の要素が無ければパスのまま。
-- フォルダ名が重なったら、後から登録した方に ` (id)` を付けて分ける。
ALTER TABLE roots ADD COLUMN name TEXT NOT NULL DEFAULT '';
ALTER TABLE roots ADD COLUMN deleted_at TEXT;

UPDATE roots
SET name = substr(path, length(rtrim(path, replace(replace(path, '/', ''), '\', ''))) + 1);

UPDATE roots SET name = path WHERE name = '';

UPDATE roots
SET name = name || ' (' || id || ')'
WHERE EXISTS (
    SELECT 1 FROM roots AS other
    WHERE other.name = roots.name COLLATE NOCASE AND other.id < roots.id
);

CREATE UNIQUE INDEX roots_name_nocase ON roots (name COLLATE NOCASE) WHERE deleted_at IS NULL;
