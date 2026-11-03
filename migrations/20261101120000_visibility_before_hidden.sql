-- 非表示 (`hidden`) にする前の公開範囲を覚えておく。非表示のコンテンツの一覧から「表示に戻す」とき、
-- 元の公開範囲に戻すため (→ `api::contents::unhide_content`)。NULL なら覚えていない (この列を足す前に
-- 非表示にしたもの) で、戻すときは `authenticated` にする。
--
-- 書き込みは、公開範囲を変える箇所 (編集・公開範囲の変更・ユーザーの削除で `private` を `hidden` にする等)
-- ごとに書かず、ここのトリガーの1箇所で行う。箇所ごとに書くと、新しく足した箇所で書き忘れるため。
-- 値の検証は `visibility` と同じく Rust の型 (`Visibility`) で行う。
ALTER TABLE contents ADD COLUMN visibility_before_hidden TEXT;

CREATE TRIGGER contents_remember_visibility_before_hidden
AFTER UPDATE OF visibility ON contents
WHEN NEW.visibility = 'hidden' AND OLD.visibility != 'hidden'
BEGIN
    UPDATE contents SET visibility_before_hidden = OLD.visibility WHERE id = NEW.id;
END;

CREATE TRIGGER contents_forget_visibility_before_hidden
AFTER UPDATE OF visibility ON contents
WHEN NEW.visibility != 'hidden' AND OLD.visibility = 'hidden'
BEGIN
    UPDATE contents SET visibility_before_hidden = NULL WHERE id = NEW.id;
END;
