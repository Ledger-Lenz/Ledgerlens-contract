import re

with open('contracts/ledgerlens-score/src/test_consensus.rs', 'r') as f:
    code = f.read()

# Add do_consensus helper after model_submission
helper = """

fn do_consensus(
    env: &Env,
    client: &LedgerLensScoreContractClient<'_>,
    wallet: &Address,
    pair: &Symbol,
    submissions: &Vec<ModelSubmission>,
    timestamp: u64,
) {
    let mut nonces = Vec::new(env);
    for i in 0..submissions.len() {
        let sub = submissions.get(i).unwrap();
        let nonce = (i as u64) + 1234;
        nonces.push_back(nonce);
        
        let mut buf = [0u8; 12];
        buf[0..4].copy_from_slice(&sub.score.to_be_bytes());
        buf[4..12].copy_from_slice(&nonce.to_be_bytes());
        let hash = env.crypto().sha256(&soroban_sdk::Bytes::from_array(env, &buf));
        client.commit_consensus(&sub.model, wallet, pair, &hash);
    }
    client.reveal_consensus(&Vec::new(env), wallet, pair, submissions, &nonces, &timestamp);
}

fn try_do_consensus(
    env: &Env,
    client: &LedgerLensScoreContractClient<'_>,
    wallet: &Address,
    pair: &Symbol,
    submissions: &Vec<ModelSubmission>,
    timestamp: u64,
) -> Result<Result<(), crate::Error>, Result<soroban_sdk::Error, soroban_sdk::Error>> {
    let mut nonces = Vec::new(env);
    for i in 0..submissions.len() {
        let sub = submissions.get(i).unwrap();
        let nonce = (i as u64) + 1234;
        nonces.push_back(nonce);
        
        let mut buf = [0u8; 12];
        buf[0..4].copy_from_slice(&sub.score.to_be_bytes());
        buf[4..12].copy_from_slice(&nonce.to_be_bytes());
        let hash = env.crypto().sha256(&soroban_sdk::Bytes::from_array(env, &buf));
        client.commit_consensus(&sub.model, wallet, pair, &hash);
    }
    client.try_reveal_consensus(&Vec::new(env), wallet, pair, submissions, &nonces, &timestamp)
}
"""

if "fn do_consensus" not in code:
    code = code.replace("}\n\n#[test]\nfn test_consensus_accepts_converging_models", "}" + helper + "\n#[test]\nfn test_consensus_accepts_converging_models")

# Replace submit_consensus_score calls
code = re.sub(r'client\.submit_consensus_score\(&Vec::new\(&env\),\s*&wallet,\s*&pair,\s*&submissions,\s*&START_TS\);',
              r'do_consensus(&env, &client, &wallet, &pair, &submissions, START_TS);', code)

code = re.sub(r'client\.try_submit_consensus_score\(&Vec::new\(&env\),\s*&wallet,\s*&pair,\s*&submissions,\s*&START_TS\);',
              r'try_do_consensus(&env, &client, &wallet, &pair, &submissions, START_TS);', code)

# Update model_submission calls to add model_address
# We need to inject Address::generate(&env) right before &wallet. Wait, or reuse an address. Let's just put Address::generate(&env) where model_address is. 
# Wait, model_address is passed as a reference: &Address::generate(&env)
code = re.sub(r'model_submission\(\s*&env,\s*&client,\s*&key,\s*&wallet,', 
              r'model_submission(\n        &env, &client, &key, &Address::generate(&env), &wallet,', code)

with open('contracts/ledgerlens-score/src/test_consensus.rs', 'w') as f:
    f.write(code)

