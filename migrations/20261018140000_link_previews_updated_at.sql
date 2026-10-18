-- リンクのカードの経過を、ページを最後に更新した日時から出す (→ docs/ui.md「リンクのカード」)。
-- 日時は標準の書き方 (schema.org の JSON-LD と microdata・OGP・Dublin Core・HTTP の Last-Modified) から読み、更新した日時が無ければ公開した日時にする。
-- 今の行は公開日時 (article:published_time) しか読んでいないので、次に一覧に出たときに取り直す。
-- 検証子も消すのは、条件付きで取り直すと 304 が返り、読み直せないため。
ALTER TABLE link_previews RENAME COLUMN published_at TO updated_at;
UPDATE link_previews SET fresh_until = 0, etag = NULL, last_modified = NULL;
