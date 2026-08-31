//! Cross-version attestation domain compatibility tests (issue #696).
//!
//! These tests lock down the *domain* of the score-attestation commitment —
//! the exact byte layout `compute_commitment` hashes (see
//! `docs/attestation-spec.md` §3). The security property under test is that an
//! attestation signed for one (model version, contract version, chain,
//! deployment) tuple can never be recomputed to the same digest for a
//! *different* tuple. If two distinct domains ever collided, a signature
//! produced for one context could be replayed against another.
//!
//! Three complementary guards are used:
//!
//! 1. **Golden layout vector.** [`serialize_preimage`] is a single, independent
//!    re-implementation of the §3 layout. [`GOLDEN_DIGEST`] pins the SHA-256 of
//!    a fully-fixed preimage (values computed offline, not by this crate), so a
//!    reorder / resize / omission in `serialize_preimage` shifts the digest and
//!    fails the assertion.
//! 2. **Production ↔ reference cross-check.** `compute_commitment` is asserted
//!    byte-for-byte equal to `serialize_preimage` across a spread of vectors, so
//!    any drift in the production layout surfaces immediately.
//! 3. **Domain-separation matrix.** For every domain field, two inputs that
//!    differ *only* in that field are shown to produce different digests —
//!    including the four the issue calls out explicitly: model version,
//!    contract version, chain (`network_id`), and deployment (contract address).

use k256::ecdsa::SigningKey;
use soroban_sdk::{
    symbol_short, testutils::Address as _, testutils::Ledger as _, Address, Bytes, BytesN, Env,
    Symbol, SymbolStr, TryFromVal, Vec,
};

use crate::{
    Error, LedgerLensScoreContract, LedgerLensScoreContractClient, MaybeScoreAttestation,
    MaybeThresholdAttestation, ScoreAttestation, ScoreAttestationInput,
};

// ── Fixtures ──────────────────────────────────────────────────────────────

fn setup<'a>() -> (Env, LedgerLensScoreContractClient<'a>, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, LedgerLensScoreContract);
    let client = LedgerLensScoreContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let service = Address::generate(&env);
    (env, client, admin, service)
}

/// A single point in the attestation domain. Every field here is folded into
/// the commitment preimage (§3); the tests below flip each one in turn.
#[derive(Clone)]
struct Vector {
    wallet: Address,
    pair: Symbol,
    score: u32,
    benford_flag: bool,
    ml_flag: bool,
    timestamp: u64,
    confidence: u32,
    model_version: u32,
    contract_id: BytesN<32>,
    contract_version: u32,
}

impl Vector {
    fn base(env: &Env) -> Self {
        Vector {
            wallet: Address::generate(env),
            pair: symbol_short!("XLM_USDC"),
            score: 42,
            benford_flag: true,
            ml_flag: false,
            timestamp: 1_700_000_000,
            confidence: 90,
            model_version: 1,
            contract_id: BytesN::from_array(env, &[7u8; 32]),
            contract_version: 4,
        }
    }
}

// ── Independent §3 serializer ────────────────────────────────────────────────

/// Total preimage width per §3: 56 + 9 + 4 + 1 + 1 + 8 + 4 + 4 + 56 + 32 + 32
/// + 4 = 211 bytes.
const PREIMAGE_LEN: usize = 211;

/// Independent re-implementation of the §3 preimage layout, taking every field
/// as raw bytes. Deliberately does **not** share code with
/// `compute_commitment`, so a reorder or omission in production surfaces as a
/// mismatch rather than passing silently.
#[allow(clippy::too_many_arguments)]
fn serialize_preimage(
    wallet_str: &[u8; 56],
    pair_bytes: &[u8],
    score: u32,
    benford: u8,
    ml: u8,
    timestamp: u64,
    confidence: u32,
    model_version: u32,
    contract_str: &[u8; 56],
    network_id: &[u8; 32],
    contract_id: &[u8; 32],
    contract_version: u32,
) -> std::vec::Vec<u8> {
    assert!(pair_bytes.len() <= 9, "asset_pair exceeds the 9-byte bound");
    let mut pair_buf = [0u8; 9];
    pair_buf[..pair_bytes.len()].copy_from_slice(pair_bytes);

    let mut buf: std::vec::Vec<u8> = std::vec::Vec::with_capacity(PREIMAGE_LEN);
    buf.extend_from_slice(wallet_str); // wallet — 56-byte StrKey ASCII
    buf.extend_from_slice(&pair_buf); // asset_pair — 9 bytes, right zero-padded
    buf.extend_from_slice(&score.to_le_bytes()); // score — u32 LE
    buf.push(benford); // benford_flag — 1 byte
    buf.push(ml); // ml_flag — 1 byte
    buf.extend_from_slice(&timestamp.to_le_bytes()); // timestamp — u64 LE
    buf.extend_from_slice(&confidence.to_le_bytes()); // confidence — u32 LE
    buf.extend_from_slice(&model_version.to_le_bytes()); // model_version — u32 LE
    buf.extend_from_slice(contract_str); // contract address — 56-byte StrKey ASCII
    buf.extend_from_slice(network_id); // network id — 32 bytes
    buf.extend_from_slice(contract_id); // contract_id — 32 bytes
    buf.extend_from_slice(&contract_version.to_le_bytes()); // contract_version — u32 LE
    buf
}

/// Builds the §3 preimage for `v` by reading the live contract address and
/// network id from `env` (exactly as `compute_commitment` does).
fn reference_preimage(env: &Env, contract: &Address, v: &Vector) -> std::vec::Vec<u8> {
    let mut wallet_str = [0u8; 56];
    v.wallet.to_string().copy_into_slice(&mut wallet_str);

    let pair_str = SymbolStr::try_from_val(env, &v.pair.to_symbol_val()).unwrap();
    let pair_bytes: &[u8] = pair_str.as_ref();

    let mut contract_str = [0u8; 56];
    env.as_contract(contract, || {
        env.current_contract_address().to_string().copy_into_slice(&mut contract_str);
    });

    serialize_preimage(
        &wallet_str,
        pair_bytes,
        v.score,
        v.benford_flag as u8,
        v.ml_flag as u8,
        v.timestamp,
        v.confidence,
        v.model_version,
        &contract_str,
        &env.ledger().network_id().to_array(),
        &v.contract_id.to_array(),
        v.contract_version,
    )
}

fn sha256(env: &Env, bytes: &[u8]) -> [u8; 32] {
    env.crypto().sha256(&Bytes::from_slice(env, bytes)).to_bytes().to_array()
}

fn reference_digest(env: &Env, contract: &Address, v: &Vector) -> [u8; 32] {
    sha256(env, &reference_preimage(env, contract, v))
}

/// Calls the contract's own `compute_commitment` "as" the deployed contract so
/// `env.current_contract_address()` and `network_id()` resolve to the live
/// values — the same digest `submit_score` recomputes.
fn digest_of(env: &Env, contract: &Address, v: &Vector) -> [u8; 32] {
    env.as_contract(contract, || {
        LedgerLensScoreContract::compute_commitment(
            env,
            &v.wallet,
            &v.pair,
            v.score,
            v.benford_flag,
            v.ml_flag,
            v.timestamp,
            v.confidence,
            v.model_version,
            &v.contract_id,
            v.contract_version,
        )
        .unwrap()
        .to_bytes()
        .to_array()
    })
}

// ── 1. Golden layout vector ─────────────────────────────────────────────────

/// SHA-256 of a fully-fixed preimage, computed offline (see the PR description
/// for the exact field values). Any change to the §3 layout — field order,
/// widths, endianness, or padding — shifts this digest. Regenerate it
/// deliberately: a diff here means the on-chain attestation domain changed and
/// every off-chain signer must be updated in lockstep.
const GOLDEN_DIGEST: [u8; 32] = [
    180, 179, 218, 207, 114, 222, 86, 231, 61, 229, 78, 206, 250, 240, 158, 39, 21, 171, 10, 241,
    232, 21, 243, 224, 225, 146, 248, 128, 119, 254, 100, 39,
];

#[test]
fn test_golden_preimage_layout() {
    let (env, _client, _, _) = setup();

    // Fully-fixed inputs matching the offline golden computation.
    let mut wallet_str = [b'A'; 56];
    wallet_str[0] = b'G';
    let mut contract_str = [b'B'; 56];
    contract_str[0] = b'C';

    let preimage = serialize_preimage(
        &wallet_str,
        b"XLM_USDC",
        42,
        1,
        0,
        1_700_000_000,
        90,
        1,
        &contract_str,
        &[9u8; 32],
        &[7u8; 32],
        4,
    );

    assert_eq!(
        preimage.len(),
        PREIMAGE_LEN,
        "attestation preimage width drifted from the documented §3 layout"
    );
    assert_eq!(
        sha256(&env, &preimage),
        GOLDEN_DIGEST,
        "attestation domain digest changed — off-chain signers must be updated in lockstep"
    );
}

#[test]
fn test_compute_commitment_matches_independent_reference() {
    let (env, client, _, _) = setup();
    // A spread of vectors, including flag and boundary permutations, so the
    // independent reference exercises every field position.
    let base = Vector::base(&env);
    let mut variants = std::vec::Vec::new();
    variants.push(base.clone());
    for (b, m) in [(false, false), (false, true), (true, true)] {
        let mut x = base.clone();
        x.benford_flag = b;
        x.ml_flag = m;
        variants.push(x);
    }
    let mut edge = base.clone();
    edge.score = u32::MAX;
    edge.timestamp = u64::MAX;
    edge.confidence = u32::MAX;
    edge.model_version = u32::MAX;
    edge.contract_version = u32::MAX;
    edge.pair = symbol_short!("A"); // shortest symbol — exercises the padding path
    variants.push(edge);

    for v in &variants {
        assert_eq!(
            digest_of(&env, &client.address, v),
            reference_digest(&env, &client.address, v),
            "compute_commitment diverged from the documented §3 byte layout"
        );
    }
}

// ── 2. Domain-separation matrix ─────────────────────────────────────────────

/// Two vectors that differ only in one domain field must never collide. Each
/// closure below mutates exactly one field.
fn assert_domain_separated(
    name: &str,
    env: &Env,
    contract: &Address,
    base: &Vector,
    mutate: impl Fn(&mut Vector),
) {
    let mut other = base.clone();
    mutate(&mut other);
    assert_ne!(
        digest_of(env, contract, base),
        digest_of(env, contract, &other),
        "domain collision: changing `{name}` did not change the commitment"
    );
}

#[test]
fn test_model_version_is_domain_separated() {
    // Explicit issue-#696 field: attestations for different model versions.
    let (env, client, _, _) = setup();
    let base = Vector::base(&env);
    assert_domain_separated("model_version", &env, &client.address, &base, |v| {
        v.model_version = v.model_version.wrapping_add(1)
    });
}

#[test]
fn test_contract_version_is_domain_separated() {
    // Explicit issue-#696 field: attestations for different contract versions.
    let (env, client, _, _) = setup();
    let base = Vector::base(&env);
    assert_domain_separated("contract_version", &env, &client.address, &base, |v| {
        v.contract_version = v.contract_version.wrapping_add(1)
    });
}

#[test]
fn test_chain_is_domain_separated() {
    // Explicit issue-#696 field: same payload on two different chains. The
    // network id is read from the executing ledger, not from the attestation,
    // so it is the strongest cross-chain binding.
    let (env, client, _, _) = setup();
    let v = Vector::base(&env);

    env.ledger().set_network_id([1u8; 32]);
    let on_chain_a = digest_of(&env, &client.address, &v);

    env.ledger().set_network_id([2u8; 32]);
    let on_chain_b = digest_of(&env, &client.address, &v);

    assert_ne!(
        on_chain_a, on_chain_b,
        "domain collision: identical payload produced the same commitment on two chains"
    );
}

#[test]
fn test_deployment_is_domain_separated() {
    // Explicit issue-#696 field: two contract instances (deployment
    // environments) sharing one signer must not accept each other's
    // attestations. The contract address is self-derived from the executor.
    let env = Env::default();
    env.mock_all_auths();
    let deploy_a = env.register_contract(None, LedgerLensScoreContract);
    let deploy_b = env.register_contract(None, LedgerLensScoreContract);
    assert_ne!(deploy_a, deploy_b);

    let v = Vector::base(&env);
    assert_ne!(
        digest_of(&env, &deploy_a, &v),
        digest_of(&env, &deploy_b, &v),
        "domain collision: two deployments produced the same commitment"
    );
}

#[test]
fn test_every_payload_field_is_domain_separated() {
    // Remaining domain fields — full coverage so no field is silently dropped
    // from the commitment.
    let (env, client, _, _) = setup();
    let c = &client.address;
    let base = Vector::base(&env);

    assert_domain_separated("wallet", &env, c, &base, |v| v.wallet = Address::generate(&env));
    assert_domain_separated("asset_pair", &env, c, &base, |v| v.pair = symbol_short!("BTC_USDC"));
    assert_domain_separated("score", &env, c, &base, |v| v.score ^= 1);
    assert_domain_separated("benford_flag", &env, c, &base, |v| v.benford_flag = !v.benford_flag);
    assert_domain_separated("ml_flag", &env, c, &base, |v| v.ml_flag = !v.ml_flag);
    assert_domain_separated("timestamp", &env, c, &base, |v| v.timestamp ^= 1);
    assert_domain_separated("confidence", &env, c, &base, |v| v.confidence ^= 1);
    assert_domain_separated("contract_id", &env, c, &base, |v| {
        v.contract_id = BytesN::from_array(&env, &[8u8; 32])
    });
}

/// Order sensitivity: two equal-width fields (`score` and `confidence`, both
/// u32-LE) with swapped values must yield different digests. If the preimage
/// were an order-independent combination (e.g. XOR/sum of fields) this would
/// collide.
#[test]
fn test_layout_is_order_sensitive() {
    let (env, client, _, _) = setup();
    let mut a = Vector::base(&env);
    a.score = 11;
    a.confidence = 22;
    let mut b = a.clone();
    b.score = 22;
    b.confidence = 11;
    assert_ne!(
        digest_of(&env, &client.address, &a),
        digest_of(&env, &client.address, &b),
        "commitment is not order-sensitive: swapped field values collided"
    );
}

// ── 3. Adversarial end-to-end failure mode ──────────────────────────────────

fn signing_key(seed: u8) -> SigningKey {
    let mut bytes = [0u8; 32];
    bytes[31] = seed;
    bytes[0] = 1;
    SigningKey::from_bytes((&bytes).into()).unwrap()
}

fn pubkey_bytes(env: &Env, key: &SigningKey) -> Bytes {
    let point = key.verifying_key().to_encoded_point(true);
    Bytes::from_slice(env, point.as_bytes())
}

fn attest(env: &Env, key: &SigningKey, v: &Vector, digest: [u8; 32]) -> ScoreAttestation {
    let (sig, recid) = key.sign_prehash_recoverable(&digest).unwrap();
    let mut sig_bytes = [0u8; 65];
    sig_bytes[..64].copy_from_slice(&sig.to_bytes());
    sig_bytes[64] = recid.to_byte();
    ScoreAttestation {
        commitment: BytesN::from_array(env, &digest),
        signature: BytesN::from_array(env, &sig_bytes),
        contract_id: v.contract_id.clone(),
        contract_version: v.contract_version,
        nonce: 0,
    }
}

/// End-to-end: an attestation validly signed for `model_version = 1` is
/// replayed with the payload's `model_version` bumped to 2. Because the model
/// version is folded into the domain, `submit_score` must recompute a
/// different digest and reject with `InvalidAttestation` — proving the
/// cross-version separation is enforced on the real submit path, not just in
/// the pure hash.
#[test]
fn test_cross_model_version_replay_rejected_end_to_end() {
    let (env, client, admin, service) = setup();
    client.initialize(&admin, &service);
    let key = signing_key(1);
    client.set_service_pubkey(&Vec::new(&env), &pubkey_bytes(&env, &key));

    let mut v = Vector::base(&env);
    // The attestation's contract_version is gate-checked against the stored
    // CONTRACT_VERSION before the digest is recomputed, so it must match.
    v.contract_version = client.get_contract_version();
    v.model_version = 1;
    // Sign honestly for model_version = 1.
    let digest_v1 = digest_of(&env, &client.address, &v);
    let attestation = attest(&env, &key, &v, digest_v1);

    // Submit the *same* signed attestation but claim model_version = 2.
    let result = client.try_submit_score(
        &Vec::new(&env),
        &v.wallet,
        &v.pair,
        &v.score,
        &v.benford_flag,
        &v.ml_flag,
        &v.timestamp,
        &v.confidence,
        &2, // ← model_version bumped; digest no longer matches
        &Some(ScoreAttestationInput {
            attestation: MaybeScoreAttestation::Some(attestation),
            threshold_attestation: MaybeThresholdAttestation::None,
            commitment: None,
        }),
    );

    assert_eq!(result, Err(Ok(Error::InvalidAttestation)));
}
