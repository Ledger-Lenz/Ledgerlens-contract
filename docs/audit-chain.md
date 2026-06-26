# Admin Governance Audit Chain

**Status:** Proposed · **Contract:** `LedgerLensScoreContract` · introduced in `CONTRACT_VERSION` 4.

The contract maintains a cryptographically verifiable audit trail of all admin governance actions via a Merkle chain. This allows off-chain operators to audit the contract's governance history without replaying every action, and provides on-chain evidence of what state changes have been authorized.

## 1. Action Hash Schema

For each admin action, a single canonical hash is computed:

```
action_hash = sha256(action_name || actor || params || timestamp)
```

Where:
- `action_name`: 8-byte Symbol (e.g., `symbol_short!("pause")`, `symbol_short!("upg_prop")`)
- `actor`: Address (the admin who performed the action), serialized via `to_xdr()`
- `params`: Variable-length parameter bytes specific to the action (e.g., new_wasm_hash for upgrades)
- `timestamp`: u64 ledger timestamp, little-endian (8 bytes)

## 2. Merkle Chain Formula

The audit root is updated after each action:

```
new_root = sha256(old_root || action_hash)
```

- Start with genesis root = `sha256([0; 32])` (all zeros) at initialization
- Each subsequent action appends to the chain

## 3. Reading the Chain

`get_admin_audit_root()` returns the current root.

To verify off-chain:
1. Fetch all admin action events from the blockchain (events emit the timestamp, actor, action name, and action-specific data)
2. Reconstruct action_hash for each event using the schema above
3. Replay the chain: `root_0 = genesis; root_i = sha256(root_{i-1} || action_hash_i)`
4. Compare the final root with the on-chain `get_admin_audit_root()`

If the roots match, the governance history is authentic and unaltered.

## 4. Genesis Root

At initialization, the audit root is set to:

```
genesis_root = sha256(0x0000000000000000000000000000000000000000000000000000000000000000)
```

(SHA-256 of 32 zero bytes)

## 5. Tracked Actions

All of the following admin state-changing functions emit audit events:

- `pause`
- `unpause`
- `propose_upgrade`
- `execute_upgrade`
- `transfer_admin`
- `accept_admin`
- `cancel_admin_transfer`
- `add_service_signer`
- `remove_service_signer`
- `set_service_threshold`
- `set_signer_rotation_ttl`
- `set_signer_rotation_grace`
- `set_service_pubkey`
- `set_aggregate_service_pubkey`
- `set_consensus_config`
- `set_reveal_window`
- `set_pair_paused`
- `veto_upgrade`
- `set_upgrade_delay`
- `set_watchlist`
- `set_escalation_threshold`
- `set_risk_threshold`
- `set_jump_threshold`
- `set_hysteresis_margin`
- `set_score_embargo`
- `revoke_all_embargoes`
- `resolve_dispute_admin`
- `set_staleness_window`
- `set_decay_rate`
- `set_cooldown`
- `set_pair_cooldown`
- `set_score_velocity_cap`
- `set_finality_buffer`
- `set_history_max_depth`
- `set_global_min_confidence`
- `set_score_delegate`
- `set_pair_weight`
- `set_pair_weight_batch`

And others. Consult the contract source for the complete list.

## 6. Off-Chain Verification Example

```python
import hashlib

def verify_audit_chain(events, on_chain_root_bytes):
    """
    Verify an audit chain by replaying events.
    
    events: list of dicts with keys:
      - action_name: str (e.g., 'pause')
      - actor_xdr: bytes (Address serialized to XDR)
      - params_bytes: bytes (action-specific parameters)
      - timestamp: int (ledger timestamp)
    on_chain_root_bytes: bytes (32-byte root from get_admin_audit_root())
    """
    root = hashlib.sha256(bytes(32)).digest()
    
    for event in events:
        action_bytes = event['action_name'].encode('ascii')
        # Pad to 8 bytes
        action_bytes = action_bytes.ljust(8, b'\0')[:8]
        
        action_preimage = (
            action_bytes +
            event['actor_xdr'] +
            event['params_bytes'] +
            event['timestamp'].to_bytes(8, 'little')
        )
        action_hash = hashlib.sha256(action_preimage).digest()
        
        chain_preimage = root + action_hash
        root = hashlib.sha256(chain_preimage).digest()
    
    return root == on_chain_root_bytes
```

## 7. Audit Trail Use Cases

- **Compliance audits**: Prove that an admin upgrade was authorized and correctly ordered
- **Incident response**: Reconstruct the sequence of governance actions leading up to an incident
- **Stakeholder transparency**: Provide auditable evidence of governance decisions
- **Fork arbitration**: If a governance dispute arises, the audit chain provides a cryptographic tie-breaker
