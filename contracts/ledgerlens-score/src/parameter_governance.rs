//! Validation and application logic for time-locked parameter change proposals.

use soroban_sdk::{symbol_short, Bytes, Env, Symbol};

use crate::constants;
use crate::errors::Error;
use crate::events;
use crate::storage;
use crate::types::ScoreVelocityCap;

/// Symbol identifying a global cooldown change (`set_cooldown`).
pub fn param_key_cooldown() -> Symbol {
    symbol_short!("cooldown")
}

/// Symbol identifying a history depth change (`set_history_max_depth`).
pub fn param_key_history_depth() -> Symbol {
    symbol_short!("hist_dep")
}

/// Symbol identifying a decay rate change (`set_decay_rate`).
pub fn param_key_decay_rate() -> Symbol {
    symbol_short!("decay_rt")
}

/// Symbol identifying a velocity cap change (`set_score_velocity_cap`).
pub fn param_key_velocity_cap() -> Symbol {
    symbol_short!("vel_cap")
}

/// Symbol identifying an upgrade delay change (`set_upgrade_delay`).
pub fn param_key_upgrade_delay() -> Symbol {
    symbol_short!("upg_dlay")
}

fn read_u64(bytes: &Bytes) -> Result<u64, Error> {
    if bytes.len() != 8 {
        return Err(Error::InvalidParameterValue);
    }
    let mut arr = [0u8; 8];
    for (i, b) in arr.iter_mut().enumerate() {
        *b = bytes.get(i as u32).unwrap();
    }
    Ok(u64::from_be_bytes(arr))
}

fn read_u32(bytes: &Bytes, offset: u32) -> Result<u32, Error> {
    if bytes.len() < offset + 4 {
        return Err(Error::InvalidParameterValue);
    }
    let mut arr = [0u8; 4];
    for (i, b) in arr.iter_mut().enumerate() {
        *b = bytes.get(offset + i as u32).unwrap();
    }
    Ok(u32::from_be_bytes(arr))
}

/// Validates that `new_value` is well-formed and within bounds for `param_key`.
pub fn validate_parameter_value(
    _env: &Env,
    param_key: &Symbol,
    new_value: &Bytes,
) -> Result<(), Error> {
    if param_key == &param_key_cooldown() {
        let secs = read_u64(new_value)?;
        if !(constants::MIN_COOLDOWN_SECS..=constants::MAX_COOLDOWN_SECS).contains(&secs) {
            return Err(Error::InvalidCooldown);
        }
        return Ok(());
    }
    if param_key == &param_key_history_depth() {
        let depth = read_u32(new_value, 0)?;
        if depth == 0 || depth > constants::MAX_HISTORY_DEPTH {
            return Err(Error::InvalidHistoryDepth);
        }
        return Ok(());
    }
    if param_key == &param_key_decay_rate() {
        if new_value.len() != 8 {
            return Err(Error::InvalidParameterValue);
        }
        let numerator = read_u32(new_value, 0)? as u64;
        let denominator = read_u32(new_value, 4)? as u64;
        if denominator == 0 {
            return Err(Error::InvalidThreshold);
        }
        let max_num = constants::MAX_DECAY_LAMBDA_NUM;
        let max_den = constants::MAX_DECAY_LAMBDA_DEN;
        if numerator
            .checked_mul(max_den)
            .map(|v| v > max_num.saturating_mul(denominator))
            .unwrap_or(true)
        {
            return Err(Error::InvalidThreshold);
        }
        return Ok(());
    }
    if param_key == &param_key_velocity_cap() {
        if new_value.len() != 5 {
            return Err(Error::InvalidParameterValue);
        }
        let enabled_byte = new_value.get(0).unwrap();
        if enabled_byte > 1 {
            return Err(Error::InvalidParameterValue);
        }
        let _points = read_u32(new_value, 1)?;
        return Ok(());
    }
    if param_key == &param_key_upgrade_delay() {
        let delay = read_u64(new_value)?;
        if !(constants::MIN_UPGRADE_DELAY_SECS..=constants::MAX_UPGRADE_DELAY_SECS).contains(&delay)
        {
            return Err(Error::InvalidUpgradeDelay);
        }
        return Ok(());
    }
    Err(Error::InvalidParameterKey)
}

/// Applies a validated parameter change to instance storage.
pub fn apply_parameter_change(
    env: &Env,
    param_key: &Symbol,
    new_value: &Bytes,
) -> Result<(), Error> {
    validate_parameter_value(env, param_key, new_value)?;

    if param_key == &param_key_cooldown() {
        let secs = read_u64(new_value)?;
        storage::set_cooldown_secs(env, secs);
        events::cooldown_updated(env, secs);
        return Ok(());
    }
    if param_key == &param_key_history_depth() {
        let depth = read_u32(new_value, 0)?;
        storage::set_history_max_depth(env, depth);
        events::history_depth_updated(env, depth);
        return Ok(());
    }
    if param_key == &param_key_decay_rate() {
        let numerator = read_u32(new_value, 0)? as u64;
        let denominator = read_u32(new_value, 4)? as u64;
        storage::set_decay_rate(env, numerator, denominator);
        events::decay_rate_updated(env, numerator, denominator);
        return Ok(());
    }
    if param_key == &param_key_velocity_cap() {
        let enabled = new_value.get(0).unwrap() == 1;
        let points = read_u32(new_value, 1)?;
        let cap = ScoreVelocityCap { enabled, points_per_hour: points };
        storage::set_score_velocity_cap(env, &cap);
        events::score_velocity_cap_set(env, enabled, points);
        return Ok(());
    }
    if param_key == &param_key_upgrade_delay() {
        let delay = read_u64(new_value)?;
        storage::set_upgrade_delay(env, delay);
        return Ok(());
    }
    Err(Error::InvalidParameterKey)
}

/// Returns the current value of a parameter without modification.
pub fn get_current_parameter_value(env: &Env, param_key: &Symbol) -> Result<Bytes, Error> {
    if param_key == &param_key_cooldown() {
        let secs = storage::get_cooldown_secs(env);
        return Ok(Bytes::from_array(env, &secs.to_be_bytes()));
    }
    if param_key == &param_key_history_depth() {
        let depth = storage::get_history_max_depth(env);
        let mut bytes = [0u8; 4];
        bytes.copy_from_slice(&depth.to_be_bytes());
        return Ok(Bytes::from_array(env, &bytes));
    }
    if param_key == &param_key_decay_rate() {
        let (num, denom) = storage::get_decay_rate(env);
        let mut bytes = [0u8; 8];
        bytes[0..4].copy_from_slice(&num.to_be_bytes());
        bytes[4..8].copy_from_slice(&denom.to_be_bytes());
        return Ok(Bytes::from_array(env, &bytes));
    }
    if param_key == &param_key_velocity_cap() {
        let cap = storage::get_score_velocity_cap(env);
        let enabled = if cap.enabled { 1u8 } else { 0u8 };
        let mut bytes = [0u8; 5];
        bytes[0] = enabled;
        bytes[1..5].copy_from_slice(&cap.points_per_hour.to_be_bytes());
        return Ok(Bytes::from_array(env, &bytes));
    }
    if param_key == &param_key_upgrade_delay() {
        let delay = storage::get_upgrade_delay(env);
        return Ok(Bytes::from_array(env, &delay.to_be_bytes()));
    }
    Err(Error::InvalidParameterKey)
}

/// Returns the set of capabilities affected by a parameter change.
fn get_affected_capabilities(env: &Env, param_key: &Symbol) -> soroban_sdk::Vec<Symbol> {
    use soroban_sdk::Vec as SorobanVec;

    let mut caps = SorobanVec::new(env);
    if param_key == &param_key_cooldown() {
        caps.push_back(symbol_short!("cooldown"));
        caps.push_back(symbol_short!("ratelimit"));
    } else if param_key == &param_key_history_depth() {
        caps.push_back(symbol_short!("history"));
        caps.push_back(symbol_short!("decay"));
    } else if param_key == &param_key_decay_rate() {
        caps.push_back(symbol_short!("decay"));
        caps.push_back(symbol_short!("score"));
    } else if param_key == &param_key_velocity_cap() {
        caps.push_back(symbol_short!("velcap"));
        caps.push_back(symbol_short!("ratelimit"));
    } else if param_key == &param_key_upgrade_delay() {
        caps.push_back(symbol_short!("upgrade"));
        caps.push_back(symbol_short!("govern"));
    }
    caps
}

/// Simulates a parameter change without applying it. Returns before/after values,
/// affected capabilities, and execution window.
pub fn simulate_parameter_change(
    env: &Env,
    param_key: &Symbol,
    new_value: &Bytes,
    proposed_at: u64,
    time_lock_secs: u64,
) -> Result<crate::types::ParameterSimulation, Error> {
    validate_parameter_value(env, param_key, new_value)?;

    let current_value = get_current_parameter_value(env, param_key)?;
    let affected = get_affected_capabilities(env, param_key);
    let exec_start = proposed_at.saturating_add(time_lock_secs);
    let exec_end = proposed_at.saturating_add(time_lock_secs.saturating_mul(2));

    Ok(crate::types::ParameterSimulation {
        param_key: param_key.clone(),
        current_value,
        new_value: new_value.clone(),
        affected_capabilities: affected,
        execution_window_start: exec_start,
        execution_window_end: exec_end,
    })
}
