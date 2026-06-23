# LedgerLens TLA+ Specification

This directory contains a formal specification of the LedgerLens smart contract's state machine written in TLA+. The specification models score writes, the embargo gate, breach counter, risk band state, and the delegation chain.

## Invariants Modelled

The following critical invariants are encoded and verified:
1. **Historical Max Monotonicity**: `hwm` never decreases.
2. **Embargo Gate Soundness**: The embargo gate blocks score modifications / evaluations when an embargo is active.
3. **Breach Counter State Machine**: The breach counter correctly increments on thresholds and resets on clean submissions or manual resets.
4. **Delegation Acyclicity**: Enforces that no cyclical score delegation loops exist.
5. **Cooldown Enforcement**: Ensures a minimum time delay between valid score submissions.
6. **Score Floor Enforcement**: Prevents high-risk wallets (those that hit `HWM_THRESHOLD`) from having their scores forced below `FLOOR_VALUE`.

## How to Install and Run TLC

TLC is the official model checker for TLA+ specifications. You can run TLC from the command line using Java.

### Prerequisites

You must have Java installed (JRE 11+ recommended).

### Running TLC

1. Download the TLA+ Tools (`tla2tools.jar`) if it isn't already present:
   ```bash
   curl -L -o tla2tools.jar https://github.com/tlaplus/tlaplus/releases/download/v1.8.0/tla2tools.jar
   ```

2. Run the TLC model checker on the specification up to depth 8 using the configuration file:
   ```bash
   java -jar tla2tools.jar -depth 8 LedgerLens.tla
   ```

### Output

TLC will explore all possible states up to a depth of 8 state transitions.
- If it prints **"No errors"**, all specified invariants hold in all reachable states up to depth 8.
- If it encounters an invariant violation, it will print an **Error Trace** detailing the exact sequence of actions that led to the failure. This trace can then be converted into a Rust unit test to confirm and patch the vulnerability in the smart contract.
