extern crate std;

use crate::{
    zk_range_proof::{Sc, SeededPrng, get_generators, compress_pt, prove_range_proof},
    LedgerLensScoreContract, LedgerLensScoreContractClient,
};
use soroban_sdk::{testutils::Address as _, Address, Bytes, BytesN, Env, Symbol, Vec};

#[test]
fn test_verify_score_range_proof_success() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, LedgerLensScoreContract);
    let client = LedgerLensScoreContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let service = Address::generate(&env);
    client.initialize(&admin, &service);

    let wallet = Address::generate(&env);
    let pair = Symbol::new(&env, "XLM_USDC");

    // Score = 40, Threshold = 50. Since 40 < 50, the proof should verify.
    let score = 40u32;
    let threshold = 50u32;
    
    let r = Sc::from_u64(987654321);
    let (g_pt, h_pt, d) = get_generators();
    
    // C = g^score * h^r
    let c_pt = g_pt.mul(Sc::from_u64(score as u64), d).add(h_pt.mul(r, d), d);
    let commitment = compress_pt(&env, &c_pt);
    std::println!("G_PT IS ON CURVE: {:?}", crate::zk_range_proof::is_on_curve(g_pt.x, g_pt.y, d));
    std::println!("C_PT IS ON CURVE: {:?}", crate::zk_range_proof::is_on_curve(c_pt.x, c_pt.y, d));

    client.submit_score(
        &Vec::new(&env),
        &wallet,
        &pair,
        &score,
        &false,
        &false,
        &1,
        &90,
        &1,
        &Some(crate::ScoreAttestationInput {
            attestation: crate::MaybeScoreAttestation::None,
            threshold_attestation: crate::MaybeThresholdAttestation::None,
            commitment: Some(commitment.clone().into()),
        }),
    );

    // Prover generates range proof showing T - 1 - v >= 0
    // T - 1 - v = 50 - 1 - 40 = 9
    // Blinding factor is -r
    let v_prime = threshold - 1 - score; // 9
    let r_prime = r.neg();
    
    let prng = SeededPrng::new([1u8; 32]);
    let proof = prove_range_proof(&env, v_prime, r_prime, prng);
    let proof_bytes = proof.to_bytes(&env);

    let result = client.verify_score_range_proof(
        &wallet,
        &pair,
        &commitment,
        &proof_bytes,
        &threshold,
    );
    assert!(result);
}

#[test]
fn test_verify_score_range_proof_invalid_threshold() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, LedgerLensScoreContract);
    let client = LedgerLensScoreContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let service = Address::generate(&env);
    client.initialize(&admin, &service);

    let wallet = Address::generate(&env);
    let pair = Symbol::new(&env, "XLM_USDC");

    // Score = 60. Try to verify proof for threshold = 50 (which requires 60 < 50, invalid).
    let score = 60u32;
    let threshold = 50u32;
    
    let r = Sc::from_u64(987654321);
    let (g_pt, h_pt, d) = get_generators();
    
    let c_pt = g_pt.mul(Sc::from_u64(score as u64), d).add(h_pt.mul(r, d), d);
    let commitment = compress_pt(&env, &c_pt);

    client.submit_score(
        &Vec::new(&env),
        &wallet,
        &pair,
        &score,
        &false,
        &false,
        &1,
        &90,
        &1,
        &Some(crate::ScoreAttestationInput {
            attestation: crate::MaybeScoreAttestation::None,
            threshold_attestation: crate::MaybeThresholdAttestation::None,
            commitment: Some(commitment.clone().into()),
        }),
    );

    // Prover tries to generate a range proof for v' = threshold - 1 - score = -11 (out of range [0, 256))
    // We pass a dummy/invalid proof or a proof generated for a different value.
    let prng = SeededPrng::new([1u8; 32]);
    // Try to prove 9 instead of -11 (which would be for score 40)
    let proof = prove_range_proof(&env, 9, r.neg(), prng);
    let proof_bytes = proof.to_bytes(&env);

    let result = client.verify_score_range_proof(
        &wallet,
        &pair,
        &commitment,
        &proof_bytes,
        &threshold,
    );
    // Should fail because the commitment C' computed on-chain won't match the proof
    assert!(!result);
}

#[test]
fn test_verify_score_range_proof_tampered_commitment() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, LedgerLensScoreContract);
    let client = LedgerLensScoreContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let service = Address::generate(&env);
    client.initialize(&admin, &service);

    let wallet = Address::generate(&env);
    let pair = Symbol::new(&env, "XLM_USDC");

    let score = 40u32;
    let threshold = 50u32;
    
    let r = Sc::from_u64(987654321);
    let (g_pt, h_pt, d) = get_generators();
    
    let c_pt = g_pt.mul(Sc::from_u64(score as u64), d).add(h_pt.mul(r, d), d);
    let commitment = compress_pt(&env, &c_pt);

    client.submit_score(
        &Vec::new(&env),
        &wallet,
        &pair,
        &score,
        &false,
        &false,
        &1,
        &90,
        &1,
        &Some(crate::ScoreAttestationInput {
            attestation: crate::MaybeScoreAttestation::None,
            threshold_attestation: crate::MaybeThresholdAttestation::None,
            commitment: Some(commitment.clone().into()),
        }),
    );

    let v_prime = threshold - 1 - score;
    let r_prime = r.neg();
    let prng = SeededPrng::new([1u8; 32]);
    let proof = prove_range_proof(&env, v_prime, r_prime, prng);
    let proof_bytes = proof.to_bytes(&env);

    // Tamper with commitment
    let mut tampered_bytes = commitment.to_array();
    tampered_bytes[0] ^= 1;
    let tampered_commitment = BytesN::from_array(&env, &tampered_bytes);

    let result = client.verify_score_range_proof(
        &wallet,
        &pair,
        &tampered_commitment,
        &proof_bytes,
        &threshold,
    );
    assert!(!result);
}

#[test]
fn test_verify_score_range_proof_tampered_proof() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, LedgerLensScoreContract);
    let client = LedgerLensScoreContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let service = Address::generate(&env);
    client.initialize(&admin, &service);

    let wallet = Address::generate(&env);
    let pair = Symbol::new(&env, "XLM_USDC");

    let score = 40u32;
    let threshold = 50u32;
    
    let r = Sc::from_u64(987654321);
    let (g_pt, h_pt, d) = get_generators();
    
    let c_pt = g_pt.mul(Sc::from_u64(score as u64), d).add(h_pt.mul(r, d), d);
    let commitment = compress_pt(&env, &c_pt);

    client.submit_score(
        &Vec::new(&env),
        &wallet,
        &pair,
        &score,
        &false,
        &false,
        &1,
        &90,
        &1,
        &Some(crate::ScoreAttestationInput {
            attestation: crate::MaybeScoreAttestation::None,
            threshold_attestation: crate::MaybeThresholdAttestation::None,
            commitment: Some(commitment.clone().into()),
        }),
    );

    let v_prime = threshold - 1 - score;
    let r_prime = r.neg();
    let prng = SeededPrng::new([1u8; 32]);
    let proof = prove_range_proof(&env, v_prime, r_prime, prng);
    let proof_bytes = proof.to_bytes(&env);

    // Tamper with proof bytes
    let mut arr = [0u8; 800];
    for i in 0..800 {
        arr[i] = proof_bytes.get(i as u32).unwrap();
    }
    arr[200] ^= 1; // tamper with one byte
    let tampered_proof = Bytes::from_array(&env, &arr);

    let result = client.verify_score_range_proof(
        &wallet,
        &pair,
        &commitment,
        &tampered_proof,
        &threshold,
    );
    assert!(!result);
}
