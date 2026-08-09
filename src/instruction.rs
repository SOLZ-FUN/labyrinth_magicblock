use pinocchio::error::ProgramError;

use crate::state::{MAX_EVENT_CHUNKS, MAX_GENERATION_RULES, MAX_PLAYERS};

pub const IX_INITIALIZE_CONFIG: u8 = 0;
pub const IX_SET_BACKEND_AUTHORITY: u8 = 1;
pub const IX_INITIALIZE_MATCH: u8 = 2;
pub const IX_DELEGATE_MATCH: u8 = 3;
pub const IX_REQUEST_MATCH_VRF: u8 = 4;
pub const IX_RESOLVE_MATCH_VRF: u8 = 5;
pub const IX_ENTER_OR_REVEAL_ROOM: u8 = 6;
pub const IX_RESOLVE_ROOM_VRF: u8 = 7;
pub const IX_RECORD_KILL: u8 = 8;
pub const IX_RECORD_DEATH: u8 = 9;
pub const IX_RECORD_RESPAWN: u8 = 10;
pub const IX_RESOLVE_PORTAL_VRF: u8 = 11;
pub const IX_RECORD_AUTHORITY_EVENT: u8 = 12;
pub const IX_RECORD_MATCH_END: u8 = 13;
pub const IX_COMMIT_MATCH: u8 = 14;
pub const IX_COMMIT_AND_UNDELEGATE_MATCH: u8 = 15;
pub const IX_INITIALIZE_MATCH_V5: u8 = 16;
pub const IX_INITIALIZE_AND_DELEGATE_EVENT_CHUNK_V5: u8 = 17;
pub const IX_DELEGATE_MATCH_V5: u8 = 18;

pub const ROSTER_ENTRY_LEN: usize = 68;
/// `initialize_match` only accepts team/party metadata on the wire. Main and
/// session wallet addresses are canonicalized from the validated profile-0
/// SessionAuthority accounts supplied alongside the instruction.
pub const INITIALIZE_ROSTER_ENTRY_LEN: usize = 4;
pub const GENERATION_RULE_LEN: usize = 8;
pub const MAX_EVENT_PAYLOAD: usize = 64;

pub const MATCH_KIND_RANKED: u8 = 0;
pub const MATCH_KIND_DIRECT: u8 = 1;
pub const LAUNCH_PERMIT_VERSION_V2: u8 = 2;
/// Legacy tag-2 permit policy retained solely for decoding v4 transactions.
pub const PAYER_POLICY_SESSION_KEY: u8 = 1;
/// A v5 bootstrap may be paid by the designated initializer's Main Wallet or
/// the corresponding existing Session-Key Wallet.
pub const PAYER_POLICY_MAIN_OR_SESSION: u8 = 1;

pub const DELEGATE_TARGET_MATCH: u8 = 0;
pub const DELEGATE_TARGET_EVENT_CHUNK: u8 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DelegateTargetArgs {
    pub target_kind: u8,
    pub match_id: [u8; 16],
    pub chunk_index: u16,
}

#[derive(Clone, Copy)]
pub struct InitializeMatchV5Args<'a> {
    pub match_id: [u8; 16],
    pub match_kind: u8,
    pub ranked_match_record: [u8; 32],
    pub er_validator: [u8; 32],
    pub roster_hash: [u8; 32],
    pub config_hash: [u8; 32],
    /// Domain-separated hash of the generation rules carried by chunk zero.
    pub generation_profile_hash: [u8; 32],
    pub config_version: u16,
    pub generation_version: u16,
    pub room_generation_profile: u16,
    pub combat_mode: u8,
    pub party_mode: u8,
    pub friendly_fire_rule: u8,
    pub respawn_enabled: u8,
    pub extraction_mode: u8,
    pub event_schema_version: u8,
    pub room_count: u16,
    pub player_count: u16,
    pub portal_unlock_kills: u32,
    pub portal_required_players: u16,
    pub portal_type_id: u16,
    pub rule_count: u8,
    pub event_chunk_count: u8,
    pub initializer_index: u8,
    pub payer_policy: u8,
    pub permit_revision: u32,
    /// Unsigned Unix timestamp (seconds), checked against Clock::unix_timestamp.
    pub expires_at: u64,
    pub roster_metadata: &'a [u8],
    /// Exact bytes signed through the preceding Ed25519 precompile.
    pub permit_payload: &'a [u8],
}

#[derive(Clone, Copy)]
pub struct InitializeEventChunkV5Args<'a> {
    pub match_id: [u8; 16],
    pub chunk_index: u16,
    pub rule_count: u8,
    pub rules: [GenerationRule; MAX_GENERATION_RULES],
    pub encoded_rules: &'a [u8],
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GenerationRule {
    pub type_id: u16,
    pub weight: u16,
    pub min_difficulty: u8,
    pub max_difficulty: u8,
    pub flags: u16,
}

#[derive(Clone, Copy)]
pub struct InitializeMatchArgs<'a> {
    pub match_id: [u8; 16],
    pub match_kind: u8,
    pub ranked_match_record: [u8; 32],
    pub config_hash: [u8; 32],
    pub generation_profile_hash: [u8; 32],
    pub config_version: u16,
    pub generation_version: u16,
    pub room_generation_profile: u16,
    pub combat_mode: u8,
    pub party_mode: u8,
    pub friendly_fire_rule: u8,
    pub respawn_enabled: u8,
    pub extraction_mode: u8,
    pub event_schema_version: u8,
    pub room_count: u16,
    pub player_count: u16,
    pub portal_unlock_kills: u32,
    pub portal_required_players: u16,
    pub portal_type_id: u16,
    pub rule_count: u8,
    pub event_chunk_count: u8,
    pub rules: [GenerationRule; MAX_GENERATION_RULES],
    pub roster: &'a [u8],
}

/// The immutable launch payload is followed by the player-paid bootstrap
/// permit. The detached backend signature must also appear in a prior Ed25519
/// precompile instruction in the same transaction.
pub struct AuthorizedInitializeMatchArgs<'a> {
    pub args: InitializeMatchArgs<'a>,
    pub permit_payload: &'a [u8],
    pub expires_slot: u64,
    pub takeover_slot: u64,
    pub er_validator: [u8; 32],
    pub finalizer_session: [u8; 32],
    pub permit_version: u8,
    pub signature: [u8; 64],
}

pub fn parse_initialize_match(data: &[u8]) -> Result<InitializeMatchArgs<'_>, ProgramError> {
    const FIXED_LEN: usize = 139;
    if data.len() < FIXED_LEN {
        return Err(ProgramError::InvalidInstructionData);
    }
    let mut offset = 0;
    let match_id = read_array(data, &mut offset)?;
    let match_kind = read_u8(data, &mut offset)?;
    let ranked_match_record = read_array(data, &mut offset)?;
    let config_hash = read_array(data, &mut offset)?;
    let generation_profile_hash = read_array(data, &mut offset)?;
    let config_version = read_u16(data, &mut offset)?;
    let generation_version = read_u16(data, &mut offset)?;
    let room_generation_profile = read_u16(data, &mut offset)?;
    let combat_mode = read_u8(data, &mut offset)?;
    let party_mode = read_u8(data, &mut offset)?;
    let friendly_fire_rule = read_u8(data, &mut offset)?;
    let respawn_enabled = read_u8(data, &mut offset)?;
    let extraction_mode = read_u8(data, &mut offset)?;
    let event_schema_version = read_u8(data, &mut offset)?;
    let room_count = read_u16(data, &mut offset)?;
    let player_count = read_u16(data, &mut offset)?;
    let portal_unlock_kills = read_u32(data, &mut offset)?;
    let portal_required_players = read_u16(data, &mut offset)?;
    let portal_type_id = read_u16(data, &mut offset)?;
    let rule_count = read_u8(data, &mut offset)?;
    let event_chunk_count = read_u8(data, &mut offset)?;

    if rule_count == 0
        || usize::from(rule_count) > MAX_GENERATION_RULES
        || event_chunk_count == 0
        || usize::from(event_chunk_count) > MAX_EVENT_CHUNKS
        || player_count == 0
        || usize::from(player_count) > MAX_PLAYERS
    {
        return Err(ProgramError::InvalidInstructionData);
    }
    let mut rules = [GenerationRule::default(); MAX_GENERATION_RULES];
    for rule in rules.iter_mut().take(usize::from(rule_count)) {
        *rule = GenerationRule {
            type_id: read_u16(data, &mut offset)?,
            weight: read_u16(data, &mut offset)?,
            min_difficulty: read_u8(data, &mut offset)?,
            max_difficulty: read_u8(data, &mut offset)?,
            flags: read_u16(data, &mut offset)?,
        };
    }
    let roster_len = usize::from(player_count)
        .checked_mul(INITIALIZE_ROSTER_ENTRY_LEN)
        .ok_or(ProgramError::InvalidInstructionData)?;
    if data.len() != offset + roster_len {
        return Err(ProgramError::InvalidInstructionData);
    }
    Ok(InitializeMatchArgs {
        match_id,
        match_kind,
        ranked_match_record,
        config_hash,
        generation_profile_hash,
        config_version,
        generation_version,
        room_generation_profile,
        combat_mode,
        party_mode,
        friendly_fire_rule,
        respawn_enabled,
        extraction_mode,
        event_schema_version,
        room_count,
        player_count,
        portal_unlock_kills,
        portal_required_players,
        portal_type_id,
        rule_count,
        event_chunk_count,
        rules,
        roster: &data[offset..],
    })
}

pub fn parse_authorized_initialize_match(
    data: &[u8],
) -> Result<AuthorizedInitializeMatchArgs<'_>, ProgramError> {
    // expiry slot, takeover slot, ER validator, designated finalizer session,
    // permit format version, detached Ed25519 signature.
    const PERMIT_FIELDS_LEN: usize = 8 + 8 + 32 + 32 + 1;
    const PERMIT_TAIL_LEN: usize = PERMIT_FIELDS_LEN + 64;
    if data.len() < PERMIT_TAIL_LEN {
        return Err(ProgramError::InvalidInstructionData);
    }
    let args_len = data.len() - PERMIT_TAIL_LEN;
    let args = parse_initialize_match(&data[..args_len])?;
    let expires_slot = u64::from_le_bytes(
        data[args_len..args_len + 8]
            .try_into()
            .map_err(|_| ProgramError::InvalidInstructionData)?,
    );
    let takeover_slot = u64::from_le_bytes(
        data[args_len + 8..args_len + 16]
            .try_into()
            .map_err(|_| ProgramError::InvalidInstructionData)?,
    );
    let er_validator = data[args_len + 16..args_len + 48]
        .try_into()
        .map_err(|_| ProgramError::InvalidInstructionData)?;
    let finalizer_session = data[args_len + 48..args_len + 80]
        .try_into()
        .map_err(|_| ProgramError::InvalidInstructionData)?;
    let permit_version = data[args_len + 80];
    let signature = data[args_len + PERMIT_FIELDS_LEN..]
        .try_into()
        .map_err(|_| ProgramError::InvalidInstructionData)?;
    Ok(AuthorizedInitializeMatchArgs {
        args,
        permit_payload: &data[..args_len + PERMIT_FIELDS_LEN],
        expires_slot,
        takeover_slot,
        er_validator,
        finalizer_session,
        permit_version,
        signature,
    })
}

pub fn parse_delegate_target(data: &[u8]) -> Result<DelegateTargetArgs, ProgramError> {
    if data.len() != 19 {
        return Err(ProgramError::InvalidInstructionData);
    }
    let target_kind = data[0];
    let match_id = data[1..17]
        .try_into()
        .map_err(|_| ProgramError::InvalidInstructionData)?;
    let chunk_index = u16::from_le_bytes([data[17], data[18]]);
    if (target_kind == DELEGATE_TARGET_MATCH && chunk_index != 0)
        || (target_kind != DELEGATE_TARGET_MATCH && target_kind != DELEGATE_TARGET_EVENT_CHUNK)
    {
        return Err(ProgramError::InvalidInstructionData);
    }
    Ok(DelegateTargetArgs {
        target_kind,
        match_id,
        chunk_index,
    })
}

/// Parses the exact payload after instruction tag 16. The detached Ed25519
/// signature is intentionally not duplicated in this instruction.
pub fn parse_initialize_match_v5(data: &[u8]) -> Result<InitializeMatchV5Args<'_>, ProgramError> {
    const FIXED_LEN: usize = 217;
    if data.len() < FIXED_LEN {
        return Err(ProgramError::InvalidInstructionData);
    }
    let permit_payload = data;
    let mut offset = 0;
    let match_id = read_array(data, &mut offset)?;
    let match_kind = read_u8(data, &mut offset)?;
    let ranked_match_record = read_array(data, &mut offset)?;
    let er_validator = read_array(data, &mut offset)?;
    let roster_hash = read_array(data, &mut offset)?;
    let config_hash = read_array(data, &mut offset)?;
    let generation_profile_hash = read_array(data, &mut offset)?;
    let config_version = read_u16(data, &mut offset)?;
    let generation_version = read_u16(data, &mut offset)?;
    let room_generation_profile = read_u16(data, &mut offset)?;
    let combat_mode = read_u8(data, &mut offset)?;
    let party_mode = read_u8(data, &mut offset)?;
    let friendly_fire_rule = read_u8(data, &mut offset)?;
    let respawn_enabled = read_u8(data, &mut offset)?;
    let extraction_mode = read_u8(data, &mut offset)?;
    let event_schema_version = read_u8(data, &mut offset)?;
    let room_count = read_u16(data, &mut offset)?;
    let player_count = read_u16(data, &mut offset)?;
    let portal_unlock_kills = read_u32(data, &mut offset)?;
    let portal_required_players = read_u16(data, &mut offset)?;
    let portal_type_id = read_u16(data, &mut offset)?;
    let rule_count = read_u8(data, &mut offset)?;
    let event_chunk_count = read_u8(data, &mut offset)?;
    let initializer_index = read_u8(data, &mut offset)?;
    let payer_policy = read_u8(data, &mut offset)?;
    let permit_revision = read_u32(data, &mut offset)?;
    let expires_at = read_u64(data, &mut offset)?;
    if offset != FIXED_LEN
        || !matches!(match_kind, MATCH_KIND_RANKED | MATCH_KIND_DIRECT)
        || payer_policy != PAYER_POLICY_MAIN_OR_SESSION
        || expires_at == 0
        || player_count == 0
        || usize::from(player_count) > MAX_PLAYERS
        || usize::from(initializer_index) >= usize::from(player_count)
        || rule_count == 0
        || usize::from(rule_count) > MAX_GENERATION_RULES
        || event_chunk_count == 0
        || usize::from(event_chunk_count) > MAX_EVENT_CHUNKS
    {
        return Err(ProgramError::InvalidInstructionData);
    }
    let roster_len = usize::from(player_count)
        .checked_mul(INITIALIZE_ROSTER_ENTRY_LEN)
        .ok_or(ProgramError::InvalidInstructionData)?;
    if data.len() != FIXED_LEN + roster_len {
        return Err(ProgramError::InvalidInstructionData);
    }
    Ok(InitializeMatchV5Args {
        match_id,
        match_kind,
        ranked_match_record,
        er_validator,
        roster_hash,
        config_hash,
        generation_profile_hash,
        config_version,
        generation_version,
        room_generation_profile,
        combat_mode,
        party_mode,
        friendly_fire_rule,
        respawn_enabled,
        extraction_mode,
        event_schema_version,
        room_count,
        player_count,
        portal_unlock_kills,
        portal_required_players,
        portal_type_id,
        rule_count,
        event_chunk_count,
        initializer_index,
        payer_policy,
        permit_revision,
        expires_at,
        roster_metadata: &data[offset..],
        permit_payload,
    })
}

pub fn parse_initialize_event_chunk_v5(
    data: &[u8],
) -> Result<InitializeEventChunkV5Args<'_>, ProgramError> {
    const FIXED_LEN: usize = 19;
    if data.len() < FIXED_LEN {
        return Err(ProgramError::InvalidInstructionData);
    }
    let mut offset = 0;
    let match_id = read_array(data, &mut offset)?;
    let chunk_index = read_u16(data, &mut offset)?;
    let rule_count = read_u8(data, &mut offset)?;
    if usize::from(rule_count) > MAX_GENERATION_RULES
        || (chunk_index == 0 && rule_count == 0)
        || (chunk_index != 0 && rule_count != 0)
    {
        return Err(ProgramError::InvalidInstructionData);
    }
    let rules_len = usize::from(rule_count)
        .checked_mul(GENERATION_RULE_LEN)
        .ok_or(ProgramError::InvalidInstructionData)?;
    if data.len() != FIXED_LEN + rules_len {
        return Err(ProgramError::InvalidInstructionData);
    }
    let encoded_rules = &data[offset..];
    let mut rules = [GenerationRule::default(); MAX_GENERATION_RULES];
    for rule in rules.iter_mut().take(usize::from(rule_count)) {
        *rule = GenerationRule {
            type_id: read_u16(data, &mut offset)?,
            weight: read_u16(data, &mut offset)?,
            min_difficulty: read_u8(data, &mut offset)?,
            max_difficulty: read_u8(data, &mut offset)?,
            flags: read_u16(data, &mut offset)?,
        };
    }
    Ok(InitializeEventChunkV5Args {
        match_id,
        chunk_index,
        rule_count,
        rules,
        encoded_rules,
    })
}

pub fn parse_delegate_match_v5(data: &[u8]) -> Result<[u8; 16], ProgramError> {
    if data.len() != 16 {
        return Err(ProgramError::InvalidInstructionData);
    }
    data.try_into()
        .map_err(|_| ProgramError::InvalidInstructionData)
}

pub fn parse_pubkey(data: &[u8]) -> Result<[u8; 32], ProgramError> {
    if data.len() != 32 {
        return Err(ProgramError::InvalidInstructionData);
    }
    data.try_into()
        .map_err(|_| ProgramError::InvalidInstructionData)
}

pub fn parse_room_id(data: &[u8]) -> Result<u16, ProgramError> {
    if data.len() != 2 {
        return Err(ProgramError::InvalidInstructionData);
    }
    Ok(u16::from_le_bytes([data[0], data[1]]))
}

pub fn parse_room_callback(data: &[u8]) -> Result<([u8; 32], u16, u64), ProgramError> {
    if data.len() != 42 {
        return Err(ProgramError::InvalidInstructionData);
    }
    let randomness = data[..32]
        .try_into()
        .map_err(|_| ProgramError::InvalidInstructionData)?;
    let room_id = u16::from_le_bytes([data[32], data[33]]);
    let nonce = u64::from_le_bytes(
        data[34..42]
            .try_into()
            .map_err(|_| ProgramError::InvalidInstructionData)?,
    );
    Ok((randomness, room_id, nonce))
}

pub fn parse_portal_callback(data: &[u8]) -> Result<([u8; 32], u64), ProgramError> {
    if data.len() != 40 {
        return Err(ProgramError::InvalidInstructionData);
    }
    let randomness = data[..32]
        .try_into()
        .map_err(|_| ProgramError::InvalidInstructionData)?;
    let nonce = u64::from_le_bytes(
        data[32..40]
            .try_into()
            .map_err(|_| ProgramError::InvalidInstructionData)?,
    );
    Ok((randomness, nonce))
}

pub fn parse_kill(data: &[u8]) -> Result<(u16, u16, u16, u64), ProgramError> {
    if data.len() != 14 {
        return Err(ProgramError::InvalidInstructionData);
    }
    Ok((
        u16::from_le_bytes([data[0], data[1]]),
        u16::from_le_bytes([data[2], data[3]]),
        u16::from_le_bytes([data[4], data[5]]),
        u64::from_le_bytes(
            data[6..14]
                .try_into()
                .map_err(|_| ProgramError::InvalidInstructionData)?,
        ),
    ))
}

pub fn parse_player_event(data: &[u8]) -> Result<(u16, u16, u64), ProgramError> {
    if data.len() != 12 {
        return Err(ProgramError::InvalidInstructionData);
    }
    Ok((
        u16::from_le_bytes([data[0], data[1]]),
        u16::from_le_bytes([data[2], data[3]]),
        u64::from_le_bytes(
            data[4..12]
                .try_into()
                .map_err(|_| ProgramError::InvalidInstructionData)?,
        ),
    ))
}

pub struct AuthorityEventArgs<'a> {
    pub event_type: u16,
    pub event_version: u8,
    pub flags: u8,
    pub actor_index: u16,
    pub target_index: u16,
    pub room_id: u16,
    pub event_id: u64,
    pub payload: &'a [u8],
}

pub fn parse_authority_event(data: &[u8]) -> Result<AuthorityEventArgs<'_>, ProgramError> {
    const HEADER_LEN: usize = 2 + 1 + 1 + 2 + 2 + 2 + 8 + 1;
    if data.len() < HEADER_LEN {
        return Err(ProgramError::InvalidInstructionData);
    }
    let payload_len = usize::from(data[18]);
    if payload_len > MAX_EVENT_PAYLOAD || data.len() != HEADER_LEN + payload_len {
        return Err(ProgramError::InvalidInstructionData);
    }
    Ok(AuthorityEventArgs {
        event_type: u16::from_le_bytes([data[0], data[1]]),
        event_version: data[2],
        flags: data[3],
        actor_index: u16::from_le_bytes([data[4], data[5]]),
        target_index: u16::from_le_bytes([data[6], data[7]]),
        room_id: u16::from_le_bytes([data[8], data[9]]),
        event_id: u64::from_le_bytes(
            data[10..18]
                .try_into()
                .map_err(|_| ProgramError::InvalidInstructionData)?,
        ),
        payload: &data[HEADER_LEN..],
    })
}

pub struct MatchEndArgs<'a> {
    pub winner_bytes: &'a [u8],
    pub result_hash: [u8; 32],
    pub event_id: u64,
}

pub fn parse_match_end(data: &[u8]) -> Result<MatchEndArgs<'_>, ProgramError> {
    if data.len() < 1 + 32 + 8 {
        return Err(ProgramError::InvalidInstructionData);
    }
    let winner_count = usize::from(data[0]);
    if winner_count == 0
        || winner_count > MAX_PLAYERS
        || data.len() != 1 + winner_count * 2 + 32 + 8
    {
        return Err(ProgramError::InvalidInstructionData);
    }
    let winner_bytes = &data[1..1 + winner_count * 2];
    let result_hash = data[1 + winner_count * 2..33 + winner_count * 2]
        .try_into()
        .map_err(|_| ProgramError::InvalidInstructionData)?;
    let event_id = u64::from_le_bytes(
        data[33 + winner_count * 2..41 + winner_count * 2]
            .try_into()
            .map_err(|_| ProgramError::InvalidInstructionData)?,
    );
    Ok(MatchEndArgs {
        winner_bytes,
        result_hash,
        event_id,
    })
}

pub fn match_end_winner_at(args: &MatchEndArgs<'_>, index: usize) -> Result<u16, ProgramError> {
    let count = args.winner_bytes.len() / 2;
    if index >= count {
        return Err(ProgramError::InvalidInstructionData);
    }
    let offset = index * 2;
    Ok(u16::from_le_bytes([
        args.winner_bytes[offset],
        args.winner_bytes[offset + 1],
    ]))
}

pub fn match_end_winner_count(args: &MatchEndArgs<'_>) -> usize {
    args.winner_bytes.len() / 2
}

fn read_u8(data: &[u8], offset: &mut usize) -> Result<u8, ProgramError> {
    let value = *data
        .get(*offset)
        .ok_or(ProgramError::InvalidInstructionData)?;
    *offset += 1;
    Ok(value)
}

fn read_u16(data: &[u8], offset: &mut usize) -> Result<u16, ProgramError> {
    let end = offset
        .checked_add(2)
        .ok_or(ProgramError::InvalidInstructionData)?;
    let value = u16::from_le_bytes(
        data.get(*offset..end)
            .ok_or(ProgramError::InvalidInstructionData)?
            .try_into()
            .map_err(|_| ProgramError::InvalidInstructionData)?,
    );
    *offset = end;
    Ok(value)
}

fn read_u32(data: &[u8], offset: &mut usize) -> Result<u32, ProgramError> {
    let end = offset
        .checked_add(4)
        .ok_or(ProgramError::InvalidInstructionData)?;
    let value = u32::from_le_bytes(
        data.get(*offset..end)
            .ok_or(ProgramError::InvalidInstructionData)?
            .try_into()
            .map_err(|_| ProgramError::InvalidInstructionData)?,
    );
    *offset = end;
    Ok(value)
}

fn read_u64(data: &[u8], offset: &mut usize) -> Result<u64, ProgramError> {
    let end = offset
        .checked_add(8)
        .ok_or(ProgramError::InvalidInstructionData)?;
    let value = u64::from_le_bytes(
        data.get(*offset..end)
            .ok_or(ProgramError::InvalidInstructionData)?
            .try_into()
            .map_err(|_| ProgramError::InvalidInstructionData)?,
    );
    *offset = end;
    Ok(value)
}

fn read_array<const N: usize>(data: &[u8], offset: &mut usize) -> Result<[u8; N], ProgramError> {
    let end = offset
        .checked_add(N)
        .ok_or(ProgramError::InvalidInstructionData)?;
    let value = data
        .get(*offset..end)
        .ok_or(ProgramError::InvalidInstructionData)?
        .try_into()
        .map_err(|_| ProgramError::InvalidInstructionData)?;
    *offset = end;
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delegate_target_is_fixed_and_rejects_invalid_combinations() {
        let mut event = [0u8; 19];
        event[0] = DELEGATE_TARGET_EVENT_CHUNK;
        event[1..17].fill(7);
        event[17..19].copy_from_slice(&4u16.to_le_bytes());
        assert_eq!(
            parse_delegate_target(&event).unwrap(),
            DelegateTargetArgs {
                target_kind: DELEGATE_TARGET_EVENT_CHUNK,
                match_id: [7; 16],
                chunk_index: 4,
            }
        );

        event[0] = DELEGATE_TARGET_MATCH;
        assert_eq!(
            parse_delegate_target(&event),
            Err(ProgramError::InvalidInstructionData)
        );
        event[17..19].fill(0);
        assert_eq!(
            parse_delegate_target(&event).unwrap().target_kind,
            DELEGATE_TARGET_MATCH
        );
    }

    #[test]
    fn ten_player_initialize_wire_uses_four_metadata_bytes_per_player() {
        // Fixed fields through the event-chunk count consume 131 bytes. Eight
        // generation rules consume 64 bytes; ten roster entries consume 40.
        // Including the instruction tag, the complete data payload is 236 B.
        let serialized_instruction_len =
            1 + 131 + 8 * GENERATION_RULE_LEN + 10 * INITIALIZE_ROSTER_ENTRY_LEN;
        assert_eq!(serialized_instruction_len, 236);
        assert!(serialized_instruction_len < 1_232);
    }

    #[test]
    fn v5_max_roster_and_rule_counts_parse_with_bounded_wires() {
        let mut init = [0u8; 217 + MAX_PLAYERS * INITIALIZE_ROSTER_ENTRY_LEN];
        init[16] = MATCH_KIND_RANKED;
        init[17] = 1; // non-zero ranked MatchRecord
        init[49] = 2; // validator
        init[177..179].copy_from_slice(&1u16.to_le_bytes());
        init[179..181].copy_from_slice(&1u16.to_le_bytes());
        init[181..183].copy_from_slice(&1u16.to_le_bytes());
        init[186] = 1;
        init[188] = 1;
        init[189..191].copy_from_slice(&64u16.to_le_bytes());
        init[191..193].copy_from_slice(&(MAX_PLAYERS as u16).to_le_bytes());
        init[193..197].copy_from_slice(&1u32.to_le_bytes());
        init[197..199].copy_from_slice(&1u16.to_le_bytes());
        init[199..201].copy_from_slice(&1u16.to_le_bytes());
        init[201] = MAX_GENERATION_RULES as u8;
        init[202] = 8;
        init[203] = (MAX_PLAYERS - 1) as u8;
        init[204] = PAYER_POLICY_MAIN_OR_SESSION;
        init[205..209].copy_from_slice(&7u32.to_le_bytes());
        init[209..217].copy_from_slice(&1_900_000_000u64.to_le_bytes());
        let parsed = parse_initialize_match_v5(&init).unwrap();
        assert_eq!(parsed.player_count, 12);
        assert_eq!(parsed.rule_count, 16);
        assert_eq!(parsed.roster_metadata.len(), 48);
        assert_eq!(init.len(), 265);

        let mut chunk = [0u8; 19 + MAX_GENERATION_RULES * GENERATION_RULE_LEN];
        chunk[18] = MAX_GENERATION_RULES as u8;
        for index in 0..MAX_GENERATION_RULES {
            let offset = 19 + index * GENERATION_RULE_LEN;
            chunk[offset..offset + 2].copy_from_slice(&((index + 1) as u16).to_le_bytes());
            chunk[offset + 2..offset + 4].copy_from_slice(&1u16.to_le_bytes());
        }
        let parsed_chunk = parse_initialize_event_chunk_v5(&chunk).unwrap();
        assert_eq!(parsed_chunk.rule_count, 16);
        assert_eq!(chunk.len(), 147);
    }

    #[test]
    fn v5_parser_rejects_payer_policy_and_length_tampering() {
        let mut init = [0u8; 217 + INITIALIZE_ROSTER_ENTRY_LEN];
        init[16] = MATCH_KIND_DIRECT;
        init[177..179].copy_from_slice(&1u16.to_le_bytes());
        init[179..181].copy_from_slice(&1u16.to_le_bytes());
        init[181..183].copy_from_slice(&1u16.to_le_bytes());
        init[189..191].copy_from_slice(&1u16.to_le_bytes());
        init[191..193].copy_from_slice(&1u16.to_le_bytes());
        init[193..197].copy_from_slice(&1u32.to_le_bytes());
        init[197..199].copy_from_slice(&1u16.to_le_bytes());
        init[199..201].copy_from_slice(&1u16.to_le_bytes());
        init[201] = 1;
        init[202] = 1;
        init[204] = PAYER_POLICY_MAIN_OR_SESSION;
        init[209..217].copy_from_slice(&1_900_000_000u64.to_le_bytes());
        assert!(parse_initialize_match_v5(&init).is_ok());
        init[204] = 9;
        assert!(parse_initialize_match_v5(&init).is_err());
        init[204] = PAYER_POLICY_MAIN_OR_SESSION;
        let original_validator = {
            let parsed = parse_initialize_match_v5(&init).unwrap();
            assert_ne!(parsed.permit_payload, &[]);
            parsed.er_validator
        };
        init[49] = 7; // validator is inside the exact signed payload.
        let changed_validator = parse_initialize_match_v5(&init).unwrap();
        assert_ne!(original_validator, changed_validator.er_validator);
        assert!(parse_initialize_match_v5(&init[..init.len() - 1]).is_err());
    }
}
