-- 公開できるフォルダの security-scoped bookmark (→ docs/design.md 13.1)。
--
-- macOS のサンドボックスでは、利用者がフォルダ選択の窓で選んだフォルダしか読めず、
-- その許可は起動し直すと消える。窓で選んだときにブックマークを作って残し、
-- 起動のたびに、登録中のフォルダの分だけ許可を戻す。
-- `roots` と分けて持つのは、窓で選んだ時点 (登録の前) に書くため。登録済みのフォルダを
-- 選び直したときも書き直すので、サンドボックスより前に登録したフォルダは選び直せば読める。
-- macOS 以外では使わない (行が無いまま)。
CREATE TABLE folder_bookmarks (
    path TEXT PRIMARY KEY NOT NULL,
    bookmark BLOB NOT NULL
);
