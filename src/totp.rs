use hmac::{Hmac, Mac};
use sha1::Sha1;
use subtle::ConstantTimeEq;

const STEP_SECONDS: u64 = 30;
const SECRET_BYTES: usize = 20;
const SECRET_CHARS: usize = 32;
const BASE32: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";

pub fn generate_secret() -> Result<String, String> {
    let mut secret = [0; SECRET_BYTES];
    getrandom::getrandom(&mut secret).map_err(|_| "Could not generate an authenticator secret.")?;
    Ok(encode_secret(&secret))
}

pub fn valid_secret(secret: &str) -> bool {
    decode_secret(secret).is_some()
}

pub fn match_counter(secret: &str, code: &str, now: u64, after: Option<u64>) -> Option<u64> {
    let code = code.as_bytes();
    if code.len() != 6 || !code.iter().all(u8::is_ascii_digit) {
        return None;
    }
    let key = decode_secret(secret)?;
    let current = now / STEP_SECONDS;
    [
        current.checked_sub(1),
        Some(current),
        current.checked_add(1),
    ]
    .into_iter()
    .flatten()
    .filter(|&counter| bool::from(counter_code(&key, counter).as_slice().ct_eq(code)))
    .max()
    .filter(|counter| after.is_none_or(|previous| *counter > previous))
}

fn counter_code(key: &[u8; SECRET_BYTES], counter: u64) -> [u8; 6] {
    let mut mac = Hmac::<Sha1>::new_from_slice(key).expect("HMAC accepts a key of any size");
    mac.update(&counter.to_be_bytes());
    let digest = mac.finalize().into_bytes();
    let offset = usize::from(digest[19] & 0x0f);
    let block = [
        digest[offset] & 0x7f,
        digest[offset + 1],
        digest[offset + 2],
        digest[offset + 3],
    ];
    let mut value = u32::from_be_bytes(block) % 1_000_000;
    let mut code = [b'0'; 6];
    for digit in code.iter_mut().rev() {
        *digit += (value % 10) as u8;
        value /= 10;
    }
    code
}

fn encode_secret(secret: &[u8; SECRET_BYTES]) -> String {
    let mut encoded = String::with_capacity(SECRET_CHARS);
    for block in secret.chunks_exact(5) {
        let bits = block
            .iter()
            .fold(0_u64, |bits, byte| (bits << 8) | u64::from(*byte));
        for shift in (0..8).rev() {
            encoded.push(char::from(BASE32[((bits >> (5 * shift)) & 31) as usize]));
        }
    }
    encoded
}

fn decode_secret(secret: &str) -> Option<[u8; SECRET_BYTES]> {
    if secret.len() != SECRET_CHARS {
        return None;
    }
    let mut key = [0; SECRET_BYTES];
    for (input, output) in secret
        .as_bytes()
        .chunks_exact(8)
        .zip(key.chunks_exact_mut(5))
    {
        let bits = input.iter().try_fold(0_u64, |bits, &symbol| {
            let value = match symbol {
                b'A'..=b'Z' => symbol - b'A',
                b'2'..=b'7' => symbol - b'2' + 26,
                _ => return None,
            };
            Some((bits << 5) | u64::from(value))
        })?;
        output.copy_from_slice(&bits.to_be_bytes()[3..]);
    }
    Some(key)
}

#[cfg(test)]
pub(crate) fn test_code(secret: &str, counter: u64) -> String {
    let key = decode_secret(secret).expect("valid test secret");
    String::from_utf8(counter_code(&key, counter).to_vec()).expect("numeric code")
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECRET: &str = "GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ";

    #[test]
    fn rfc_6238_sha1_vectors_use_the_last_six_digits() {
        assert_eq!(encode_secret(b"12345678901234567890"), SECRET);
        assert_eq!(
            decode_secret(SECRET).as_ref(),
            Some(b"12345678901234567890")
        );
        for (time, code) in [
            (59, "287082"),
            (1_111_111_109, "081804"),
            (1_111_111_111, "050471"),
            (1_234_567_890, "005924"),
            (2_000_000_000, "279037"),
            (20_000_000_000, "353130"),
        ] {
            assert_eq!(test_code(SECRET, time / 30), code);
            assert_eq!(match_counter(SECRET, code, time, None), Some(time / 30));
        }
    }

    #[test]
    fn accepts_only_unused_codes_within_one_step() {
        assert_eq!(match_counter(SECRET, "755224", 0, None), Some(0));
        assert_eq!(match_counter(SECRET, "287082", 29, None), Some(1));
        assert_eq!(match_counter(SECRET, "287082", 60, Some(0)), Some(1));
        assert_eq!(match_counter(SECRET, "287082", 60, Some(1)), None);
        assert_eq!(match_counter(SECRET, "287082", 90, None), None);
        assert_eq!(match_counter(SECRET, "0287082", 59, None), None);
        assert_eq!(match_counter(SECRET, "૨87082", 59, None), None);
        assert!(!valid_secret("GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJ="));
        assert!(valid_secret(&generate_secret().unwrap()));
    }
}
