-- 国内は直接、海外は Managed Payments で売る (→ docs/pro.md「アカウントと販売の窓口」)。
-- 予約に、どちらで支払いの画面を作ったかを残す。頼み直しでも同じ中身で頼むため (Stripe は中身の違う頼み直しを断る)。
ALTER TABLE checkouts ADD COLUMN managed_payments INTEGER NOT NULL DEFAULT 0;

-- 台帳に、MP の取引かどうかとカードの発行国を残す。売上の区分のため。振り分けや返金には使わない。
ALTER TABLE entitlements ADD COLUMN stripe_managed_payments INTEGER;
ALTER TABLE entitlements ADD COLUMN stripe_card_country TEXT;

-- 付けるのは支払いごとに1つ。Checkout Session ごとの1つに加えて、支払いの id でも重複を許さない。
DROP INDEX entitlements_payment_intent;
CREATE UNIQUE INDEX entitlements_payment_intent ON entitlements (stripe_payment_intent_id);

-- 受けた webhook の知らせ。処理し終えたものは送り直されても処理し直さず、失敗したものは Stripe の送り直しで処理し直す。
CREATE TABLE stripe_events (
    id TEXT PRIMARY KEY,
    type TEXT NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('received', 'done', 'failed')),
    received_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);
