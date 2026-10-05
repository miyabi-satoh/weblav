//! 結び付きの符号 (→ docs/pro.md「符号の作り方」)。窓口の側は `account-server/src/link.ts`。

use hkdf::Hkdf;
use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;

use super::{DAY, Plan};
use crate::api::hex_encode as hex;

/// 申し込みの版。形を変えるときに上げる。
const REQUEST_VERSION: u8 = 1;
/// 返しのコードの日付の起点 (2026-01-01 UTC) の UNIX 日。
const EPOCH_DAY: i64 = 20_454;
/// 発行日から期限までの日数を入れるビット数。窓口の `SPAN_BITS` と同じ。
const SPAN_BITS: u32 = 9;
/// 返しのコードの符号の長さ (ビット)。窓口の `GRANT_TAG_BITS` と同じ。
const GRANT_TAG_BITS: u32 = 49;
/// 返しのコードの頭 (プラン1ビット・発行日16ビット・日数9ビット) のビット数。
const HEAD_BITS: usize = 26;

const CROCKFORD: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

type HmacSha256 = Hmac<Sha256>;

fn hmac(key: &[u8], parts: &[&[u8]]) -> [u8; 32] {
    let mut mac = HmacSha256::new_from_slice(key).expect("HMAC は長さを問わない");
    for part in parts {
        mac.update(part);
    }
    mac.finalize()
        .into_bytes()
        .as_slice()
        .try_into()
        .expect("SHA-256 は32バイト")
}

fn bit(bytes: &[u8], i: usize) -> u8 {
    if i < bytes.len() * 8 {
        (bytes[i >> 3] >> (7 - (i & 7))) & 1
    } else {
        0
    }
}

/// 5ビットずつ Crockford の base32 にする。末尾の足りないビットは 0 で埋める。
fn encode_base32(bytes: &[u8], bits: usize) -> String {
    (0..bits)
        .step_by(5)
        .map(|i| {
            let v = (0..5).fold(0usize, |v, j| (v << 1) | bit(bytes, i + j) as usize);
            CROCKFORD[v] as char
        })
        .collect()
}

/// 打ち込まれた base32 を読む。区切り・空白を除き、`O`→`0`・`I`/`L`→`1` に読み替える。
fn decode_base32(text: &str, bits: usize) -> Option<Vec<u8>> {
    let chars: Vec<u8> = text
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '-')
        .map(|c| match c.to_ascii_uppercase() {
            'O' => '0',
            'I' | 'L' => '1',
            c => c,
        } as u8)
        .collect();
    if chars.len() != bits.div_ceil(5) {
        return None;
    }
    let mut out = vec![0u8; bits.div_ceil(8)];
    let mut pos = 0;
    for ch in chars {
        let v = CROCKFORD.iter().position(|&c| c == ch)? as u8;
        for j in (0..5).rev() {
            let b = (v >> j) & 1;
            if pos < bits {
                out[pos >> 3] |= b << (7 - (pos & 7));
            } else if b != 0 {
                return None;
            }
            pos += 1;
        }
    }
    Some(out)
}

/// 窓口の公開鍵と組み合わせて導いた秘密と、WebLAV の公開鍵。共有の値が全部 0 なら `None`。
pub fn derive_secret(
    link_kid: u8,
    server_public: &[u8; 32],
    client_secret: [u8; 32],
) -> Option<([u8; 32], [u8; 32])> {
    let client_public = x25519_dalek::x25519(client_secret, x25519_dalek::X25519_BASEPOINT_BYTES);
    let shared = x25519_dalek::x25519(client_secret, *server_public);
    if shared.iter().all(|&b| b == 0) {
        return None;
    }
    let mut info = Vec::with_capacity(65);
    info.push(link_kid);
    info.extend_from_slice(&client_public);
    info.extend_from_slice(server_public);
    let mut secret = [0u8; 32];
    Hkdf::<Sha256>::new(Some(b"weblav-link-v1"), &shared)
        .expand(&info, &mut secret)
        .expect("32バイトは HKDF-SHA256 の出せる長さ");
    Some((secret, client_public))
}

/// 結び付きの id (16進16文字)。
pub fn installation_id(secret: &[u8; 32]) -> String {
    hex(&hmac(secret, &[b"weblav-installation-id"])[..8])
}

/// 窓口に確かめるときの認証 (16進)。
pub fn check_auth(secret: &[u8; 32]) -> String {
    hex(&hmac(secret, &[b"weblav-check"]))
}

/// 同じ WebLAV の結び直しの印。前の秘密で、新しい公開鍵に付ける。
fn relink_tag(previous_secret: &[u8; 32], new_public: &[u8; 32]) -> [u8; 8] {
    hmac(previous_secret, &[b"weblav-relink", new_public])[..8]
        .try_into()
        .expect("8バイト")
}

/// 申し込みの文字列。`previous` は、結んである WebLAV の結び直しのときの前の秘密。
pub fn format_request(
    link_kid: u8,
    created_at: i64,
    public: &[u8; 32],
    previous: Option<&[u8; 32]>,
) -> String {
    let mut bytes = vec![REQUEST_VERSION, link_kid];
    bytes.extend_from_slice(&((created_at / 60) as u32).to_be_bytes());
    bytes.extend_from_slice(public);
    if let Some(previous) = previous {
        bytes.extend_from_slice(&unhex(&installation_id(previous)));
        bytes.extend_from_slice(&relink_tag(previous, public));
    }
    encode_base32(&bytes, bytes.len() * 8)
}

/// 外した証し (23文字)。
pub fn release_code(secret: &[u8; 32]) -> String {
    let mut bytes = unhex(&installation_id(secret));
    bytes.extend_from_slice(&hmac(secret, &[b"weblav-release"])[..6]);
    encode_base32(&bytes, bytes.len() * 8)
}

/// 返しのコードの中身。時刻は UNIX 秒。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Grant {
    pub plan: Plan,
    /// 発行日の始まり。
    pub issued_at: i64,
    /// 期限の日の終わり。
    pub expires_at: i64,
}

/// 返しのコードを確かめて読む。符号が合わなければ `None`。
pub fn parse_grant(secret: &[u8; 32], code: &str) -> Option<Grant> {
    let bits = HEAD_BITS + GRANT_TAG_BITS as usize;
    let bytes = decode_base32(code, bits)?;
    let read = |from: usize, len: usize| -> u64 {
        (from..from + len).fold(0u64, |v, i| (v << 1) | bit(&bytes, i) as u64)
    };
    let head = read(0, HEAD_BITS) as u32;
    let tag = read(HEAD_BITS, GRANT_TAG_BITS as usize);
    let expected = hmac(secret, &[b"weblav-grant", &head.to_be_bytes()]);
    let expected =
        (0..GRANT_TAG_BITS as usize).fold(0u64, |v, i| (v << 1) | bit(&expected, i) as u64);
    // 1つの整数の比較なので、分岐の時間で何ビット合ったかは漏れない。
    if (tag ^ expected) != 0 {
        return None;
    }
    let plan = if head >> 25 == 1 {
        Plan::Organization
    } else {
        Plan::Personal
    };
    let issued_day = EPOCH_DAY + i64::from((head >> SPAN_BITS) & 0xffff);
    let span = i64::from(head & ((1 << SPAN_BITS) - 1));
    Some(Grant {
        plan,
        issued_at: issued_day * DAY,
        expires_at: (issued_day + span + 1) * DAY,
    })
}

/// 窓口と同じ作り方の返しのコード (テスト用)。
#[cfg(test)]
pub(crate) fn format_grant(
    secret: &[u8; 32],
    plan: Plan,
    issued_at: i64,
    expires_at: i64,
) -> String {
    let issued_day = issued_at.div_euclid(DAY) - EPOCH_DAY;
    let span = expires_at.div_euclid(DAY) - EPOCH_DAY - issued_day;
    let head: u32 = (u32::from(plan == Plan::Organization) << 25)
        | ((issued_day as u32) << SPAN_BITS)
        | span as u32;
    let tag = hmac(secret, &[b"weblav-grant", &head.to_be_bytes()]);
    let bits = HEAD_BITS + GRANT_TAG_BITS as usize;
    let mut bytes = vec![0u8; bits.div_ceil(8)];
    for i in 0..bits {
        let b = if i < HEAD_BITS {
            ((head >> (HEAD_BITS - 1 - i)) & 1) as u8
        } else {
            bit(&tag, i - HEAD_BITS)
        };
        bytes[i >> 3] |= b << (7 - (i & 7));
    }
    let code = encode_base32(&bytes, bits);
    format!("{}-{}-{}", &code[..5], &code[5..10], &code[10..])
}

fn unhex(text: &str) -> Vec<u8> {
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).expect("自分で作った16進"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 窓口 (`account-server/test/link.test.ts`) と同じ入力から、同じ値が出ること。
    /// 値を変えたら、窓口のテストの値も合わせる。
    #[test]
    fn matches_the_account_server_vectors() {
        let server_public: [u8; 32] =
            unhex("01bfddeda9867a5136a2e0d7e5b68edbb827c04d0f540f07d7f992136bf92e7b")
                .try_into()
                .expect("32バイト");
        let (secret, public) =
            derive_secret(0, &server_public, [7u8; 32]).expect("秘密を導けなかった");
        assert_eq!(
            hex(&public),
            "13be4feaeaf204c7fd3358fc9c00721881d174278128227ec674f37f7fe97b6d"
        );
        assert_eq!(
            hex(&secret),
            "1ace5b59d960e76fbb90933fbf772723d51f15fd6649c90ff2d616fe1f53c4fc"
        );
        assert_eq!(installation_id(&secret), "9d1e343f38716e3c");
        assert_eq!(
            check_auth(&secret),
            "428e868e000884ee74ca2eed88e121ac8a173ab4962a56866a79bd5d64d53925"
        );
        assert_eq!(
            format_request(0, 1_791_090_000, &public, None),
            "04003HVZDG9VWKZAXBS09HZX6DCFS700E8C83MBM4Y0JG8KYRSTF6ZVZX5XPT"
        );
        assert_eq!(
            format_request(0, 1_791_090_000, &public, Some(&[9u8; 32])),
            "04003HVZDG9VWKZAXBS09HZX6DCFS700E8C83MBM4Y0JG8KYRSTF6ZVZX5XPTKA3SRZHADTDFY68F1ST9NXK7X8"
        );
        assert_eq!(release_code(&secret), "KMF38FSRE5Q3RN0RHA5SMCR");
    }

    /// 窓口が作った返しのコード (`account-server/test/link.test.ts` の `grant`) を読める。
    #[test]
    fn parses_the_account_server_grant() {
        let secret: [u8; 32] =
            unhex("1ace5b59d960e76fbb90933fbf772723d51f15fd6649c90ff2d616fe1f53c4fc")
                .try_into()
                .expect("32バイト");
        let issued_day = 1_791_090_000 / DAY;
        assert_eq!(
            parse_grant(&secret, "g250f cmyg3 d78aw"),
            Some(Grant {
                plan: Plan::Organization,
                issued_at: issued_day * DAY,
                expires_at: (issued_day + 31) * DAY,
            })
        );
        assert_eq!(parse_grant(&secret, "G250F-CMYG3-D78AX"), None);
        assert_eq!(parse_grant(&[0u8; 32], "G250F-CMYG3-D78AW"), None);
    }

    #[test]
    fn base32_round_trips_and_reads_loosely() {
        let bytes = [0xde, 0xad, 0xbe, 0xef, 0x01];
        let text = encode_base32(&bytes, 40);
        assert_eq!(decode_base32(&text, 40).expect("読めなかった"), bytes);
        let loose = text.to_lowercase().replace('0', "o").replace('1', "l");
        assert_eq!(
            decode_base32(&format!("{}-{}", &loose[..4], &loose[4..]), 40).expect("読めなかった"),
            bytes
        );
        assert!(decode_base32(&text[1..], 40).is_none());
    }
}
