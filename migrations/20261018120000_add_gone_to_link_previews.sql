-- 最後に確かめたとき、ページが無くなっていた (404・410) か。AI が集めたリンクから外すのに使う (→ docs/ai.md「AI が集めるリンク」)。
-- 検索の業者の索引には消えたページが残っていることがあり、押すまで分からない行を並べないため。
-- 今ある行は、次に取り直すまで分からないので 0 のままにする。
ALTER TABLE link_previews ADD COLUMN gone INTEGER NOT NULL DEFAULT 0;
