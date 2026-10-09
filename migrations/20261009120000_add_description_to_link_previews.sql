-- リンクの詳しい表示に、ページの説明を出す (→ docs/ui.md「リンクのカード」)。
--
-- 覚えている行は、次に一覧に出たときに取り直させる。説明はまだ取っておらず、
-- 画像も詳しい表示には小さい大きさ (長い辺 640px) で縮めてあるため。
-- 検証子を消して、304 で前の情報のまま残らないようにし、画像の元の URL を消して、画像も縮め直させる。
-- 置いてある画像のファイルは、取り直すまで今のカードに使い、取り直したときに入れ替わって消える。
ALTER TABLE link_previews ADD COLUMN description TEXT;

UPDATE link_previews
SET etag = NULL, last_modified = NULL, fresh_until = 0, checked_at = 0, image_source = NULL;
