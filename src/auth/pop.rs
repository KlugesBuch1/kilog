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
        self.sign_request_at(url, body, windows_timestamp_now())
    }

    fn sign_request_at(&self, url: &str, body: &str, timestamp: u64) -> String {
        let payload = signature_payload(timestamp, &path_and_query(url), body);
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

fn signature_payload(timestamp: u64, path_and_query: &str, body: &str) -> Vec<u8> {
    let signed = format!("POST\0{path_and_query}\0\0{body}\0");
    let signed = signed.as_bytes();
    let mut payload = vec![0u8; 14 + signed.len()];
    payload[..4].copy_from_slice(&POLICY_VERSION.to_be_bytes());
    payload[5..13].copy_from_slice(&timestamp.to_be_bytes());
    payload[14..].copy_from_slice(signed);
    payload
}
