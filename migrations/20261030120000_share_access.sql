-- 共有 (公開できるフォルダー) ごとに、一般ユーザーがその中で何をできるかを持つ (→ `api::shares`)。
-- フォルダーの中のファイルを、画面から見る・足す・動かす・消すようにするため。
-- 管理者は常にすべての操作ができるので、ここでは持たない。
--
-- 値は 'none' (見えない)・'read' (見る・ダウンロード)・'add' (見るに加えて、足す・コピーで入れる)・
-- 'all' (すべての操作)。検証は Rust の `ShareAccess` で行う (CHECK を付けると、値を足すたびに
-- テーブルを作り直すことになるため)。
--
-- 既定と今ある行は 'read'。これまでも、ログインした一般ユーザーは公開できるフォルダーの中を
-- 選んでコンテンツに登録できたので、できることを減らさない。
ALTER TABLE roots ADD COLUMN editor_access TEXT NOT NULL DEFAULT 'read';

-- ユーザーごとの例外。行があれば、そのユーザーには `roots.editor_access` の代わりにこちらを使う。
-- 共有やユーザーを消すと一緒に消える (共有は削除済みの印を付けて行を残すので、登録し直すと例外も戻る)。
CREATE TABLE root_user_access (
    root_id INTEGER NOT NULL REFERENCES roots (id) ON DELETE CASCADE,
    user_id INTEGER NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    access TEXT NOT NULL,
    PRIMARY KEY (root_id, user_id)
);
