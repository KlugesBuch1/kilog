use base64::Engine;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use p256::ecdsa::SigningKey;
use p256::ecdsa::signature::Signer;

const WINDOWS_EPOCH_OFFSET: u64 = 11_644_473_600;
const POLICY_VERSION: u32 = 1;

pub struct ProofSigner {
    key: SigningKey,
    proof_key: serde_json::Value,
}

impl ProofSigner {
    pub fn generate() -> Self {
        let key = SigningKey::random(&mut rand_core::OsRng);
        let point = key.verifying_key().to_encoded_point(false);
        let proof_key = serde_json::json!({
            "kty": "EC",
            "x": base64url(&pad_coord(point.x().expect("p-256 x"))),
            "y": base64url(&pad_coord(point.y().expect("p-256 y"))),
            "crv": "P-256",
            "alg": "ES256",
            "use": "sig",
        });
        Self { key, proof_key }
    }

    pub fn proof_key(&self) -> serde_json::Value {
        self.proof_key.clone()
    }

    pub fn sign_request(&self, url: &str, body: &str) -> String {
        self.sign_bytes("POST", url, body.as_bytes())
    }

    pub fn sign_bytes(&self, method: &str, url: &str, body: &[u8]) -> String {
        let timestamp = windows_timestamp_now();
        let payload = signature_payload(timestamp, method, &path_and_query(url), body);
        let signature: p256::ecdsa::Signature = self.key.sign(&payload);
        let mut header = Vec::with_capacity(12 + 64);
        header.extend_from_slice(&POLICY_VERSION.to_be_bytes());
        header.extend_from_slice(&timestamp.to_be_bytes());
        header.extend_from_slice(&signature.to_bytes());
        STANDARD.encode(header)
    }
}

fn pad_coord(coord: &[u8]) -> [u8; 32] {
    let mut padded = [0u8; 32];
    let start = 32 - coord.len();
    padded[start..].copy_from_slice(coord);
    padded
}

fn base64url(bytes: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(bytes)
}

fn path_and_query(url: &str) -> String {
    let url = reqwest::Url::parse(url).expect("xbox auth url");
    match url.query() {
        Some(query) => format!("{}?{query}", url.path()),
        None => url.path().to_owned(),
    }
}

fn windows_timestamp_now() -> u64 {
    let unix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0);
    windows_timestamp(unix)
}

fn windows_timestamp(unix_seconds: u64) -> u64 {
    unix_seconds
        .saturating_add(WINDOWS_EPOCH_OFFSET)
        .saturating_mul(10_000_000)
}

fn signature_payload(timestamp: u64, method: &str, path_and_query: &str, body: &[u8]) -> Vec<u8> {
    let mut signed = format!("{method}\0{path_and_query}\0\0").into_bytes();
    signed.extend_from_slice(body);
    signed.push(0);
    let mut payload = vec![0u8; 14 + signed.len()];
    payload[..4].copy_from_slice(&POLICY_VERSION.to_be_bytes());
    payload[5..13].copy_from_slice(&timestamp.to_be_bytes());
    payload[14..].copy_from_slice(&signed);
    payload
}

#[cfg(test)]
mod tests {
    use super::signature_payload;

    #[test]
    fn signature_payload_matches_the_xbox_layout() {
        let post = signature_payload(1, "POST", "/path", b"{}");
        let signed = "POST\0/path\0\0{}\0";
        let mut expected = vec![0u8; 14 + signed.len()];
        expected[..4].copy_from_slice(&1u32.to_be_bytes());
        expected[5..13].copy_from_slice(&1u64.to_be_bytes());
        expected[14..].copy_from_slice(signed.as_bytes());
        assert_eq!(post, expected);

        let put = signature_payload(1, "PUT", "/users/me/gamerpic", &[1, 2, 0, 3]);
        assert_eq!(&put[put.len() - 5..], &[1, 2, 0, 3, 0]);
    }
}
