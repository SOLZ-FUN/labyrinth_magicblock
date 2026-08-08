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

pub const ROSTER_ENTRY_LEN: usize = 68;
pub const GENERATION_RULE_LEN: usize = 8;
pub const MAX_EVENT_PAYLOAD: usize = 64;

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

pub fn parse_initialize_match(data: &[u8]) -> Result<InitializeMatchArgs<'_>, ProgramError> {
    const FIXED_LEN: usize = 138;
    if data.len() < FIXED_LEN {
        return Err(ProgramError::InvalidInstructionData);
    }
    let mut offset = 0;
    let match_id = read_array(data, &mut offset)?;
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
        .checked_mul(ROSTER_ENTRY_LEN)
        .ok_or(ProgramError::InvalidInstructionData)?;
    if data.len() != offset + roster_len {
        return Err(ProgramError::InvalidInstructionData);
    }
    Ok(InitializeMatchArgs {
        match_id,
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
