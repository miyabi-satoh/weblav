-- AI を使う機能 (軸の定義を AI に送る・AI が集めるリンク・簡易チャット) を外し、その表と列を消す。
-- 提供側に費用が掛かる機能を最初の版に入れないことにしたため。軸の定義は、プロンプトを外部の AI に貼る流れだけが残る。
-- 子の表から消す (`ai_links`・`ai_link_exclusions` は `ai_link_groups` を指す)。索引は表と一緒に消える。

DROP TABLE ai_link_exclusions;
DROP TABLE ai_links;
DROP TABLE ai_link_groups;
DROP TABLE ai_usage;
ALTER TABLE users DROP COLUMN ai_allowed;
