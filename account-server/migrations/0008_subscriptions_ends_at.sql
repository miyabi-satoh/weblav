-- 解約の手続きを済ませたサブスクが終わる日時 (UNIX 秒) を持つ (→ docs/pro.md「窓口の画面」)。
-- アカウントのページで、次の更新日なのか、解約済みでその日に終わるのかを出し分けるため。
-- 解約していなければ NULL。Stripe の知らせ (customer.subscription.updated) のたびに、取り直したサブスクで合わせる。
ALTER TABLE subscriptions ADD COLUMN ends_at INTEGER;
