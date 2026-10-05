-- Stripe で買った Pro (→ docs/pro.md「アカウントと販売の窓口」)。
-- webhook は送り直されるので、Checkout Session ごとに1行にして二重に付けない。返金されたら支払いで探して消す。
ALTER TABLE entitlements ADD COLUMN stripe_checkout_session_id TEXT;
ALTER TABLE entitlements ADD COLUMN stripe_payment_intent_id TEXT;
CREATE UNIQUE INDEX entitlements_checkout_session ON entitlements (stripe_checkout_session_id);
CREATE INDEX entitlements_payment_intent ON entitlements (stripe_payment_intent_id);

-- 返金・不審請求で Pro を外した支払い。知らせの届く順は決まっていないので、付ける知らせが後から届いても付けないように残す。
CREATE TABLE stripe_revoked_payments (
    payment_intent_id TEXT PRIMARY KEY,
    created_at INTEGER NOT NULL
);

-- 支払いの画面の予約。アカウントごとに1行で、期限までは同じ画面へ送り、2つのタブや買い直しで二重に払わせないように。
-- Stripe に頼む前に取り、id を Idempotency-Key にする (同時に押されても Stripe で画面が1つになる)。
-- url は Stripe が画面を作ってから入る。頼み直しても Stripe が同じ頼みと見るよう、戻り先 (next・lang) も予約に残す。
CREATE TABLE checkouts (
    id TEXT PRIMARY KEY,
    account_id TEXT NOT NULL UNIQUE REFERENCES accounts (id),
    next TEXT NOT NULL,
    lang TEXT NOT NULL,
    url TEXT,
    expires_at INTEGER NOT NULL
);
