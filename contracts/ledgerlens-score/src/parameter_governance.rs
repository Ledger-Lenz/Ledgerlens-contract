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
pub fn validate_parameter_value(_env: &Env, param_key: &Symbol, new_value: &Bytes) -> Result<(), Error> {
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
        if numerator.checked_mul(max_den).map(|v| v > max_num.saturating_mul(denominator)).unwrap_or(true) {
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
pub fn apply_parameter_change(env: &Env, param_key: &Symbol, new_value: &Bytes) -> Result<(), Error> {
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
