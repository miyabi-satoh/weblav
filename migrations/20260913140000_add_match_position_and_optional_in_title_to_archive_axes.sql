-- 軸ごとに、ファイル名の語を照合する位置と、値が無くても表示タイトルを組み立てるかを持つ
-- (→ docs/design.md 3.2・3.4)。
--
-- 旧課程や表紙のような付け足しの語を別の軸に分け、科目 × 旧課程 × 表紙の組み合わせを
-- 辞書に1行ずつ登録しなくて済むようにするため。既存の軸はこれまでどおりの動きにする。
ALTER TABLE archive_axes ADD COLUMN match_position TEXT NOT NULL DEFAULT 'anywhere';
ALTER TABLE archive_axes ADD COLUMN optional_in_title INTEGER NOT NULL DEFAULT 0;
