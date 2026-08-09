use pinocchio::{error::ProgramError, ProgramResult};
use solana_sha256_hasher::hashv;

use crate::{
    error::LabyrinthError,
    events::{EventBatch, MatchEvent, MAX_EVENTS_PER_BATCH},
    instruction::{
        GenerationRule, InitializeMatchArgs, InitializeMatchV5Args, MATCH_KIND_DIRECT,
        MATCH_KIND_RANKED, MAX_EVENT_PAYLOAD, ROSTER_ENTRY_LEN,
    },
};

pub const LEGACY_PROGRAM_VERSION: u8 = 4;
pub const PROGRAM_VERSION: u8 = 5;
pub const BOOTSTRAP_SCHEMA_V2: u8 = 2;
pub const BOOTSTRAP_FLAG_INITIALIZED: u8 = 1 << 0;
pub const BOOTSTRAP_FLAG_ALL_CHUNKS_READY: u8 = 1 << 1;
pub const MAX_PLAYERS: usize = 12;
pub const MAX_ROOMS: usize = 64;
pub const MAX_GENERATION_RULES: usize = 16;
/// Each event chunk is delegated independently, avoiding account-list limits
/// while retaining 384 durable events across the configured chunk set.
pub const MAX_EVENT_CHUNKS: usize = 12;
pub const MAX_EVENTS_PER_CHUNK: usize = 32;

pub const PHASE_AWAITING_MATCH_VRF: u8 = 1;
pub const PHASE_ACTIVE: u8 = 2;
pub const PHASE_TERMINAL: u8 = 3;

pub const VRF_UNINITIALIZED: u8 = 0;
pub const VRF_PENDING: u8 = 1;
pub const VRF_RESOLVED: u8 = 2;

pub const PORTAL_LOCKED: u8 = 0;
pub const PORTAL_PENDING_VRF: u8 = 1;
pub const PORTAL_SPAWNED: u8 = 2;

pub const ROOM_UNDEFINED: u8 = 0;
pub const ROOM_PENDING: u8 = 1;
pub const ROOM_RESOLVED: u8 = 2;

pub const EVENT_MATCH_INITIALIZED: u16 = 1;
pub const EVENT_MATCH_VRF_REQUESTED: u16 = 2;
pub const EVENT_MATCH_VRF_RESOLVED: u16 = 3;
pub const EVENT_PLAYER_SPAWN_ASSIGNED: u16 = 100;
pub const EVENT_ROOM_REVEAL_REQUESTED: u16 = 200;
pub const EVENT_ROOM_RESOLVED: u16 = 201;
pub const EVENT_PLAYER_KILLED: u16 = 300;
pub const EVENT_PLAYER_DIED: u16 = 301;
pub const EVENT_PLAYER_RESPAWNED: u16 = 302;
pub const EVENT_PORTAL_UNLOCKED: u16 = 400;
pub const EVENT_PORTAL_SPAWNED: u16 = 401;
pub const EVENT_PORTAL_ACTIVATED: u16 = 402;
pub const EVENT_PLAYER_EXTRACTED: u16 = 500;
pub const EVENT_MATCH_FINISHED: u16 = 900;

pub const CONFIG_LEN: usize = 98;
pub const MATCH_HEADER_LEN: usize = 544;
pub const PLAYER_ROW_LEN: usize = 80;
pub const ROOM_ROW_LEN: usize = 56;
pub const PLAYERS_OFFSET: usize = MATCH_HEADER_LEN;
pub const ROOMS_OFFSET: usize = PLAYERS_OFFSET + MAX_PLAYERS * PLAYER_ROW_LEN;
pub const MATCH_LEN: usize = ROOMS_OFFSET + MAX_ROOMS * ROOM_ROW_LEN;

const H_INITIALIZED: usize = 0;
const H_BUMP: usize = 1;
const H_VERSION: usize = 2;
const H_PHASE: usize = 3;
const H_BACKEND: usize = 4;
const H_RANKED_PROGRAM: usize = 36;
const H_RANKED_MATCH_RECORD: usize = 68;
const H_MATCH_ID: usize = 100;
const H_CONFIG_HASH: usize = 116;
const H_GENERATION_PROFILE_HASH: usize = 148;
const H_CONFIG_VERSION: usize = 180;
const H_GENERATION_VERSION: usize = 182;
const H_ROOM_GENERATION_PROFILE: usize = 184;
const H_COMBAT_MODE: usize = 186;
const H_PARTY_MODE: usize = 187;
const H_FRIENDLY_FIRE_RULE: usize = 188;
const H_RESPAWN_ENABLED: usize = 189;
const H_EXTRACTION_MODE: usize = 190;
const H_EVENT_SCHEMA_VERSION: usize = 191;
const H_ROOM_COUNT: usize = 192;
const H_PLAYER_COUNT: usize = 194;
const H_PORTAL_UNLOCK_KILLS: usize = 196;
const H_PORTAL_REQUIRED_PLAYERS: usize = 200;
const H_PORTAL_TYPE_ID: usize = 202;
const H_RULE_COUNT: usize = 204;
const H_EVENT_CHUNK_COUNT: usize = 205;
const H_MATCH_VRF_STATE: usize = 206;
const H_PORTAL_STATE: usize = 207;
const H_NEXT_VRF_NONCE: usize = 208;
const H_MATCH_VRF_NONCE: usize = 216;
const H_MATCH_SEED: usize = 224;
const H_PORTAL_VRF_NONCE: usize = 256;
const H_PORTAL_SEED: usize = 264;
const H_PORTAL_ROOM: usize = 296;
const H_TOTAL_KILLS: usize = 298;
const H_EVENT_SEQUENCE: usize = 302;
const H_LAST_SERVER_EVENT_ID: usize = 310;
const H_WINNER_COUNT: usize = 318;
const H_PRIMARY_WINNER: usize = 320;
const H_TERMINAL_RESULT_HASH: usize = 322;
const H_WINNERS: usize = 354;
const H_ER_VALIDATOR: usize = 378;
const H_MATCH_KIND: usize = 410;
// Reserved header bytes are now part of the player-paid bootstrap state. They
// fit before the immutable rule table, so the deployed account size remains
// stable across the v4 -> v5 authority change.
const H_BOOTSTRAP_SCHEMA: usize = 411;
const H_INITIALIZER_INDEX: usize = 412;
const H_DELEGATED_CHUNK_MASK: usize = 413;
const H_BOOTSTRAP_FLAGS: usize = 415;
const H_RULES: usize = 416;
const RULE_ROW_LEN: usize = 8;
const RULES_HASH_DOMAIN: &[u8] = b"solz-labyrinth-rules-v1";

const P_MAIN: usize = 0;
const P_SESSION: usize = 32;
const P_TEAM_ID: usize = 64;
const P_PARTY_ID: usize = 66;
const P_INITIAL_SPAWN_ROOM: usize = 68;
const P_KILLS: usize = 70;
const P_DEATHS: usize = 72;
const P_RESPAWNS: usize = 74;

const R_STATUS: usize = 0;
const R_TYPE_ID: usize = 2;
const R_DIFFICULTY: usize = 4;
const R_FIRST_REVEALER: usize = 6;
const R_VRF_NONCE: usize = 8;
const R_SEED: usize = 16;
const R_RESOLUTION_SEQUENCE: usize = 48;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ConfigState {
    pub bump: u8,
    pub authority: [u8; 32],
    pub backend_authority: [u8; 32],
    pub ranked_program: [u8; 32],
}

impl ConfigState {
    pub fn unpack(data: &[u8]) -> Result<Self, ProgramError> {
        if data.len() < CONFIG_LEN || data[0] != 1 {
            return Err(ProgramError::UninitializedAccount);
        }
        Ok(Self {
            bump: data[1],
            authority: read_array(data, 2)?,
            backend_authority: read_array(data, 34)?,
            ranked_program: read_array(data, 66)?,
        })
    }

    pub fn pack(&self, data: &mut [u8]) -> Result<(), ProgramError> {
        if data.len() < CONFIG_LEN {
            return Err(ProgramError::AccountDataTooSmall);
        }
        data[..CONFIG_LEN].fill(0);
        data[0] = 1;
        data[1] = self.bump;
        data[2..34].copy_from_slice(&self.authority);
        data[34..66].copy_from_slice(&self.backend_authority);
        data[66..98].copy_from_slice(&self.ranked_program);
        Ok(())
    }
}

pub struct InitializeMatch<'a> {
    pub bump: u8,
    pub backend_authority: [u8; 32],
    pub ranked_program: [u8; 32],
    pub er_validator: [u8; 32],
    pub finalizer_index: u8,
    pub bootstrap_permit_version: u8,
    pub args: InitializeMatchArgs<'a>,
}

pub struct MatchState<'a> {
    data: &'a mut [u8],
}

pub struct VrfRequest {
    pub nonce: u64,
    pub events: EventBatch,
}

pub struct RoomRevealRequest {
    pub nonce: Option<u64>,
    pub events: EventBatch,
}

pub struct KillRecord {
    pub portal_vrf_nonce: Option<u64>,
    pub events: EventBatch,
}

impl<'a> MatchState<'a> {
    pub fn initialize(
        data: &'a mut [u8],
        input: InitializeMatch<'_>,
    ) -> Result<Self, ProgramError> {
        validate_match_input(&input.args)?;
        if data.len() < MATCH_LEN {
            return Err(ProgramError::AccountDataTooSmall);
        }
        data[..MATCH_LEN].fill(0);
        data[H_INITIALIZED] = 1;
        data[H_BUMP] = input.bump;
        data[H_VERSION] = PROGRAM_VERSION;
        data[H_PHASE] = PHASE_AWAITING_MATCH_VRF;
        data[H_BACKEND..H_BACKEND + 32].copy_from_slice(&input.backend_authority);
        data[H_RANKED_PROGRAM..H_RANKED_PROGRAM + 32].copy_from_slice(&input.ranked_program);
        data[H_RANKED_MATCH_RECORD..H_RANKED_MATCH_RECORD + 32]
            .copy_from_slice(&input.args.ranked_match_record);
        data[H_MATCH_KIND] = input.args.match_kind;
        data[H_MATCH_ID..H_MATCH_ID + 16].copy_from_slice(&input.args.match_id);
        data[H_CONFIG_HASH..H_CONFIG_HASH + 32].copy_from_slice(&input.args.config_hash);
        data[H_GENERATION_PROFILE_HASH..H_GENERATION_PROFILE_HASH + 32]
            .copy_from_slice(&input.args.generation_profile_hash);
        write_u16(data, H_CONFIG_VERSION, input.args.config_version)?;
        write_u16(data, H_GENERATION_VERSION, input.args.generation_version)?;
        write_u16(
            data,
            H_ROOM_GENERATION_PROFILE,
            input.args.room_generation_profile,
        )?;
        data[H_COMBAT_MODE] = input.args.combat_mode;
        data[H_PARTY_MODE] = input.args.party_mode;
        data[H_FRIENDLY_FIRE_RULE] = input.args.friendly_fire_rule;
        data[H_RESPAWN_ENABLED] = input.args.respawn_enabled;
        data[H_EXTRACTION_MODE] = input.args.extraction_mode;
        data[H_EVENT_SCHEMA_VERSION] = input.args.event_schema_version;
        write_u16(data, H_ROOM_COUNT, input.args.room_count)?;
        write_u16(data, H_PLAYER_COUNT, input.args.player_count)?;
        write_u32(data, H_PORTAL_UNLOCK_KILLS, input.args.portal_unlock_kills)?;
        write_u16(
            data,
            H_PORTAL_REQUIRED_PLAYERS,
            input.args.portal_required_players,
        )?;
        write_u16(data, H_PORTAL_TYPE_ID, input.args.portal_type_id)?;
        data[H_RULE_COUNT] = input.args.rule_count;
        data[H_EVENT_CHUNK_COUNT] = input.args.event_chunk_count;
        data[H_MATCH_VRF_STATE] = VRF_UNINITIALIZED;
        data[H_PORTAL_STATE] = PORTAL_LOCKED;
        data[H_ER_VALIDATOR..H_ER_VALIDATOR + 32].copy_from_slice(&input.er_validator);
        data[H_BOOTSTRAP_SCHEMA] = input.bootstrap_permit_version;
        data[H_INITIALIZER_INDEX] = input.finalizer_index;
        write_u16(data, H_DELEGATED_CHUNK_MASK, 0)?;
        data[H_BOOTSTRAP_FLAGS] = BOOTSTRAP_FLAG_INITIALIZED;
        write_u16(data, H_PORTAL_ROOM, u16::MAX)?;
        write_u16(data, H_PRIMARY_WINNER, u16::MAX)?;
        for (index, rule) in input.args.rules.iter().enumerate() {
            write_rule(data, index, *rule)?;
        }
        for index in 0..usize::from(input.args.player_count) {
            let source = index * ROSTER_ENTRY_LEN;
            let target = player_offset(index)?;
            data[target + P_MAIN..target + P_MAIN + 32]
                .copy_from_slice(&input.args.roster[source..source + 32]);
            data[target + P_SESSION..target + P_SESSION + 32]
                .copy_from_slice(&input.args.roster[source + 32..source + 64]);
            write_u16(
                data,
                target + P_TEAM_ID,
                u16::from_le_bytes([
                    input.args.roster[source + 64],
                    input.args.roster[source + 65],
                ]),
            )?;
            write_u16(
                data,
                target + P_PARTY_ID,
                u16::from_le_bytes([
                    input.args.roster[source + 66],
                    input.args.roster[source + 67],
                ]),
            )?;
            write_u16(data, target + P_INITIAL_SPAWN_ROOM, u16::MAX)?;
        }
        Ok(Self { data })
    }

    /// Initializes only the canonical Match account. Generation rules are
    /// intentionally installed by chunk zero so account rent/delegation can be
    /// resumed one transaction at a time.
    pub fn initialize_v5(
        data: &'a mut [u8],
        bump: u8,
        backend_authority: [u8; 32],
        ranked_program: [u8; 32],
        args: InitializeMatchV5Args<'_>,
        canonical_roster: &[u8],
    ) -> Result<Self, ProgramError> {
        let mut placeholder_rules = [GenerationRule::default(); MAX_GENERATION_RULES];
        for (index, rule) in placeholder_rules
            .iter_mut()
            .take(usize::from(args.rule_count))
            .enumerate()
        {
            rule.type_id = (index + 1) as u16;
            rule.weight = 1;
        }
        let common = InitializeMatchArgs {
            match_id: args.match_id,
            match_kind: args.match_kind,
            ranked_match_record: args.ranked_match_record,
            config_hash: args.config_hash,
            generation_profile_hash: args.generation_profile_hash,
            config_version: args.config_version,
            generation_version: args.generation_version,
            room_generation_profile: args.room_generation_profile,
            combat_mode: args.combat_mode,
            party_mode: args.party_mode,
            friendly_fire_rule: args.friendly_fire_rule,
            respawn_enabled: args.respawn_enabled,
            extraction_mode: args.extraction_mode,
            event_schema_version: args.event_schema_version,
            room_count: args.room_count,
            player_count: args.player_count,
            portal_unlock_kills: args.portal_unlock_kills,
            portal_required_players: args.portal_required_players,
            portal_type_id: args.portal_type_id,
            rule_count: args.rule_count,
            event_chunk_count: args.event_chunk_count,
            rules: placeholder_rules,
            roster: canonical_roster,
        };
        let state = Self::initialize(
            data,
            InitializeMatch {
                bump,
                backend_authority,
                ranked_program,
                er_validator: args.er_validator,
                finalizer_index: args.initializer_index,
                bootstrap_permit_version: BOOTSTRAP_SCHEMA_V2,
                args: common,
            },
        )?;
        // The placeholder only lets us share the strict immutable-config and
        // roster initialization path. No placeholder rule leaves this ix.
        state.data[H_RULES..H_RULES + MAX_GENERATION_RULES * RULE_ROW_LEN].fill(0);
        Ok(state)
    }

    pub fn load(data: &'a mut [u8]) -> Result<Self, ProgramError> {
        validate_match(data)?;
        Ok(Self { data })
    }

    pub fn initialized_events(&mut self) -> Result<EventBatch, ProgramError> {
        let mut events = EventBatch::new();
        let mut payload = [0u8; MAX_EVENT_PAYLOAD];
        payload[..2].copy_from_slice(&self.room_count().to_le_bytes());
        payload[2..4].copy_from_slice(&self.player_count().to_le_bytes());
        self.emit(
            &mut events,
            EVENT_MATCH_INITIALIZED,
            1,
            0,
            u16::MAX,
            u16::MAX,
            u16::MAX,
            &payload[..4],
        )?;
        Ok(events)
    }

    pub fn phase(&self) -> u8 {
        self.data[H_PHASE]
    }

    pub fn version(&self) -> u8 {
        self.data[H_VERSION]
    }

    pub fn bump(&self) -> u8 {
        self.data[H_BUMP]
    }

    pub fn match_id(&self) -> [u8; 16] {
        self.data[H_MATCH_ID..H_MATCH_ID + 16]
            .try_into()
            .expect("fixed match id range")
    }

    pub fn match_kind(&self) -> u8 {
        self.data[H_MATCH_KIND]
    }

    pub fn backend_authority(&self) -> [u8; 32] {
        self.data[H_BACKEND..H_BACKEND + 32]
            .try_into()
            .expect("fixed backend range")
    }

    pub fn player_count(&self) -> u16 {
        read_u16(self.data, H_PLAYER_COUNT).unwrap_or_default()
    }

    pub fn room_count(&self) -> u16 {
        read_u16(self.data, H_ROOM_COUNT).unwrap_or_default()
    }

    pub fn event_chunk_count(&self) -> usize {
        usize::from(self.data[H_EVENT_CHUNK_COUNT])
    }

    pub fn er_validator(&self) -> [u8; 32] {
        self.data[H_ER_VALIDATOR..H_ER_VALIDATOR + 32]
            .try_into()
            .expect("fixed ER validator range")
    }

    pub fn finalizer_index(&self) -> u8 {
        self.data[H_INITIALIZER_INDEX]
    }

    pub fn bootstrap_permit_version(&self) -> u8 {
        self.data[H_BOOTSTRAP_SCHEMA]
    }

    pub fn delegated_chunk_mask(&self) -> u16 {
        read_u16(self.data, H_DELEGATED_CHUNK_MASK).unwrap_or_default()
    }

    pub fn is_finalizer_authority(&self, authority: &[u8; 32]) -> Result<bool, ProgramError> {
        let index = usize::from(self.finalizer_index());
        if index >= usize::from(self.player_count()) {
            return Err(LabyrinthError::InvalidConfig.into());
        }
        let offset = player_offset(index)?;
        Ok(
            &self.data[offset + P_MAIN..offset + P_MAIN + 32] == authority
                || &self.data[offset + P_SESSION..offset + P_SESSION + 32] == authority,
        )
    }

    pub fn mark_event_chunk_delegated(&mut self, index: u16) -> Result<bool, ProgramError> {
        if self.version() != PROGRAM_VERSION || usize::from(index) >= self.event_chunk_count() {
            return Err(LabyrinthError::InvalidConfig.into());
        }
        let bit = 1u16
            .checked_shl(u32::from(index))
            .ok_or(LabyrinthError::InvalidConfig)?;
        let current = self.delegated_chunk_mask();
        if current & bit != 0 {
            return Ok(false);
        }
        // A caller cannot skip a rent-bearing chunk and delegate Match early.
        if current != bit.saturating_sub(1) {
            return Err(LabyrinthError::DelegationIncomplete.into());
        }
        let next = current | bit;
        write_u16(self.data, H_DELEGATED_CHUNK_MASK, next)?;
        if next == self.full_chunk_mask() {
            self.data[H_BOOTSTRAP_FLAGS] |= BOOTSTRAP_FLAG_ALL_CHUNKS_READY;
        }
        Ok(true)
    }

    pub fn require_all_event_chunks_delegated(&self) -> Result<(), ProgramError> {
        if self.version() != PROGRAM_VERSION
            || self.delegated_chunk_mask() != self.full_chunk_mask()
            || self.data[H_BOOTSTRAP_FLAGS] & BOOTSTRAP_FLAG_ALL_CHUNKS_READY == 0
        {
            return Err(LabyrinthError::DelegationIncomplete.into());
        }
        Ok(())
    }

    pub fn full_chunk_mask(&self) -> u16 {
        (1u16 << self.event_chunk_count()).saturating_sub(1)
    }

    pub fn bootstrap_flags(&self) -> u8 {
        self.data[H_BOOTSTRAP_FLAGS]
    }

    pub fn find_authority(&self, authority: &[u8; 32]) -> Result<u16, ProgramError> {
        for index in 0..usize::from(self.player_count()) {
            let offset = player_offset(index)?;
            if &self.data[offset + P_MAIN..offset + P_MAIN + 32] == authority
                || &self.data[offset + P_SESSION..offset + P_SESSION + 32] == authority
            {
                return Ok(index as u16);
            }
        }
        Err(LabyrinthError::UnauthorizedSession.into())
    }

    pub fn matches_v5_initialization(
        &self,
        args: &InitializeMatchV5Args<'_>,
        canonical_roster: &[u8],
    ) -> Result<bool, ProgramError> {
        if self.version() != PROGRAM_VERSION
            || self.bootstrap_permit_version() != BOOTSTRAP_SCHEMA_V2
            || self.match_id() != args.match_id
            || self.match_kind() != args.match_kind
            || self.er_validator() != args.er_validator
            || self.finalizer_index() != args.initializer_index
            || self.data[H_RANKED_MATCH_RECORD..H_RANKED_MATCH_RECORD + 32]
                != args.ranked_match_record
            || self.data[H_CONFIG_HASH..H_CONFIG_HASH + 32] != args.config_hash
            || self.data[H_GENERATION_PROFILE_HASH..H_GENERATION_PROFILE_HASH + 32]
                != args.generation_profile_hash
            || read_u16(self.data, H_CONFIG_VERSION)? != args.config_version
            || read_u16(self.data, H_GENERATION_VERSION)? != args.generation_version
            || read_u16(self.data, H_ROOM_GENERATION_PROFILE)? != args.room_generation_profile
            || self.data[H_COMBAT_MODE] != args.combat_mode
            || self.data[H_PARTY_MODE] != args.party_mode
            || self.data[H_FRIENDLY_FIRE_RULE] != args.friendly_fire_rule
            || self.data[H_RESPAWN_ENABLED] != args.respawn_enabled
            || self.data[H_EXTRACTION_MODE] != args.extraction_mode
            || self.data[H_EVENT_SCHEMA_VERSION] != args.event_schema_version
            || self.room_count() != args.room_count
            || self.player_count() != args.player_count
            || read_u32(self.data, H_PORTAL_UNLOCK_KILLS)? != args.portal_unlock_kills
            || read_u16(self.data, H_PORTAL_REQUIRED_PLAYERS)? != args.portal_required_players
            || read_u16(self.data, H_PORTAL_TYPE_ID)? != args.portal_type_id
            || self.data[H_RULE_COUNT] != args.rule_count
            || self.data[H_EVENT_CHUNK_COUNT] != args.event_chunk_count
            || canonical_roster.len() != usize::from(args.player_count) * ROSTER_ENTRY_LEN
        {
            return Ok(false);
        }
        for index in 0..usize::from(args.player_count) {
            let source = index * ROSTER_ENTRY_LEN;
            let target = player_offset(index)?;
            if self.data[target + P_MAIN..target + P_MAIN + 32]
                != canonical_roster[source..source + 32]
                || self.data[target + P_SESSION..target + P_SESSION + 32]
                    != canonical_roster[source + 32..source + 64]
                || read_u16(self.data, target + P_TEAM_ID)?
                    != u16::from_le_bytes([
                        canonical_roster[source + 64],
                        canonical_roster[source + 65],
                    ])
                || read_u16(self.data, target + P_PARTY_ID)?
                    != u16::from_le_bytes([
                        canonical_roster[source + 66],
                        canonical_roster[source + 67],
                    ])
            {
                return Ok(false);
            }
        }
        Ok(true)
    }

    pub fn install_generation_rules(
        &mut self,
        rule_count: u8,
        rules: &[GenerationRule],
        encoded_rules: &[u8],
    ) -> ProgramResult {
        if self.version() != PROGRAM_VERSION
            || self.delegated_chunk_mask() != 0
            || rule_count != self.data[H_RULE_COUNT]
            || rules.len() != usize::from(rule_count)
            || encoded_rules.len() != usize::from(rule_count) * RULE_ROW_LEN
        {
            return Err(LabyrinthError::InvalidConfig.into());
        }
        let count = [rule_count];
        let actual_hash = hashv(&[RULES_HASH_DOMAIN, &count, encoded_rules]).to_bytes();
        if self.data[H_GENERATION_PROFILE_HASH..H_GENERATION_PROFILE_HASH + 32] != actual_hash {
            return Err(LabyrinthError::InvalidConfig.into());
        }
        for left in 0..usize::from(rule_count) {
            let rule = rules[left];
            if rule.type_id == 0 || rule.weight == 0 || rule.min_difficulty > rule.max_difficulty {
                return Err(LabyrinthError::InvalidConfig.into());
            }
            for right in 0..left {
                if rule.type_id == rules[right].type_id {
                    return Err(LabyrinthError::InvalidConfig.into());
                }
            }
            write_rule(self.data, left, rule)?;
        }
        Ok(())
    }

    pub fn find_session(&self, session: &[u8; 32]) -> Result<u16, ProgramError> {
        for index in 0..usize::from(self.player_count()) {
            let offset = player_offset(index)? + P_SESSION;
            if &self.data[offset..offset + 32] == session {
                return Ok(index as u16);
            }
        }
        Err(LabyrinthError::UnauthorizedSession.into())
    }

    pub fn request_match_vrf(&mut self, requester: u16) -> Result<VrfRequest, ProgramError> {
        if self.phase() != PHASE_AWAITING_MATCH_VRF {
            return Err(LabyrinthError::InvalidPhase.into());
        }
        match self.data[H_MATCH_VRF_STATE] {
            VRF_UNINITIALIZED => {}
            VRF_PENDING => return Err(LabyrinthError::AlreadyRequested.into()),
            _ => return Err(LabyrinthError::RandomnessAlreadyResolved.into()),
        }
        let nonce = self.next_vrf_nonce()?;
        write_u64(self.data, H_MATCH_VRF_NONCE, nonce)?;
        self.data[H_MATCH_VRF_STATE] = VRF_PENDING;
        let mut events = EventBatch::new();
        self.emit_u64(
            &mut events,
            EVENT_MATCH_VRF_REQUESTED,
            requester,
            u16::MAX,
            u16::MAX,
            nonce,
        )?;
        Ok(VrfRequest { nonce, events })
    }

    pub fn resolve_match_vrf(&mut self, randomness: [u8; 32]) -> Result<EventBatch, ProgramError> {
        if self.phase() != PHASE_AWAITING_MATCH_VRF || self.data[H_MATCH_VRF_STATE] != VRF_PENDING {
            return Err(LabyrinthError::InvalidVrfCallback.into());
        }
        self.data[H_MATCH_SEED..H_MATCH_SEED + 32].copy_from_slice(&randomness);
        let spawn_seed = derive(&randomness, b"spawn");
        assign_unique_spawns(
            self.data,
            usize::from(self.player_count()),
            usize::from(self.room_count()),
            spawn_seed,
        )?;
        self.data[H_MATCH_VRF_STATE] = VRF_RESOLVED;
        self.data[H_PHASE] = PHASE_ACTIVE;
        let mut events = EventBatch::new();
        self.emit(
            &mut events,
            EVENT_MATCH_VRF_RESOLVED,
            1,
            0,
            u16::MAX,
            u16::MAX,
            u16::MAX,
            &[],
        )?;
        Ok(events)
    }

    /// Emits a bounded portion of the deterministic spawn assignment history.
    /// `resolve_match_vrf` invokes this through the callback processor in
    /// consecutive batches so every assigned spawn remains durable without a
    /// large stack allocation on sBPF.
    pub fn spawn_assignment_events(
        &mut self,
        first_player: usize,
    ) -> Result<EventBatch, ProgramError> {
        if self.phase() != PHASE_ACTIVE || first_player > usize::from(self.player_count()) {
            return Err(LabyrinthError::InvalidPhase.into());
        }
        let mut events = EventBatch::new();
        let last_player = core::cmp::min(
            first_player.saturating_add(MAX_EVENTS_PER_BATCH),
            usize::from(self.player_count()),
        );
        for index in first_player..last_player {
            let room = read_u16(self.data, player_offset(index)? + P_INITIAL_SPAWN_ROOM)?;
            self.emit_u16(
                &mut events,
                EVENT_PLAYER_SPAWN_ASSIGNED,
                index as u16,
                u16::MAX,
                room,
                room,
            )?;
        }
        Ok(events)
    }

    pub fn enter_or_reveal_room(
        &mut self,
        room_id: u16,
        requester: u16,
    ) -> Result<RoomRevealRequest, ProgramError> {
        if self.phase() != PHASE_ACTIVE || room_id >= self.room_count() {
            return Err(LabyrinthError::InvalidRoom.into());
        }
        let offset = room_offset(usize::from(room_id))?;
        match self.data[offset + R_STATUS] {
            ROOM_RESOLVED | ROOM_PENDING => {
                return Ok(RoomRevealRequest {
                    nonce: None,
                    events: EventBatch::new(),
                });
            }
            ROOM_UNDEFINED => {}
            _ => return Err(LabyrinthError::InvalidRoom.into()),
        }
        let nonce = self.next_vrf_nonce()?;
        self.data[offset + R_STATUS] = ROOM_PENDING;
        write_u16(self.data, offset + R_FIRST_REVEALER, requester)?;
        write_u64(self.data, offset + R_VRF_NONCE, nonce)?;
        let mut events = EventBatch::new();
        self.emit_u64(
            &mut events,
            EVENT_ROOM_REVEAL_REQUESTED,
            requester,
            u16::MAX,
            room_id,
            nonce,
        )?;
        Ok(RoomRevealRequest {
            nonce: Some(nonce),
            events,
        })
    }

    pub fn resolve_room_vrf(
        &mut self,
        randomness: [u8; 32],
        room_id: u16,
        nonce: u64,
    ) -> Result<EventBatch, ProgramError> {
        if self.phase() != PHASE_ACTIVE || room_id >= self.room_count() {
            return Err(LabyrinthError::InvalidVrfCallback.into());
        }
        let offset = room_offset(usize::from(room_id))?;
        if self.data[offset + R_STATUS] != ROOM_PENDING
            || read_u64(self.data, offset + R_VRF_NONCE)? != nonce
        {
            return Err(LabyrinthError::InvalidVrfCallback.into());
        }
        let rule = self.choose_generation_rule(&derive(&randomness, b"room-type"))?;
        let difficulty_seed = derive(&randomness, b"difficulty");
        let difficulty = difficulty_from_rule(rule, &difficulty_seed);
        let revealer = read_u16(self.data, offset + R_FIRST_REVEALER)?;
        self.data[offset + R_STATUS] = ROOM_RESOLVED;
        write_u16(self.data, offset + R_TYPE_ID, rule.type_id)?;
        write_u16(self.data, offset + R_DIFFICULTY, difficulty)?;
        self.data[offset + R_SEED..offset + R_SEED + 32].copy_from_slice(&randomness);
        let mut events = EventBatch::new();
        let mut payload = [0u8; MAX_EVENT_PAYLOAD];
        payload[..2].copy_from_slice(&rule.type_id.to_le_bytes());
        payload[2..4].copy_from_slice(&difficulty.to_le_bytes());
        let sequence = self.emit(
            &mut events,
            EVENT_ROOM_RESOLVED,
            1,
            rule.flags as u8,
            revealer,
            u16::MAX,
            room_id,
            &payload[..4],
        )?;
        write_u64(self.data, offset + R_RESOLUTION_SEQUENCE, sequence)?;
        Ok(events)
    }

    pub fn record_kill(
        &mut self,
        killer: u16,
        victim: u16,
        room_id: u16,
        event_id: u64,
    ) -> Result<KillRecord, ProgramError> {
        if self.phase() != PHASE_ACTIVE
            || killer == victim
            || killer >= self.player_count()
            || victim >= self.player_count()
            || room_id >= self.room_count()
        {
            return Err(LabyrinthError::InvalidEvent.into());
        }
        if self.data[H_FRIENDLY_FIRE_RULE] == 0 && self.same_team(killer, victim)? {
            return Err(LabyrinthError::FriendlyFireDisabled.into());
        }
        if !self.accept_server_event_id(event_id)? {
            return Ok(KillRecord {
                portal_vrf_nonce: None,
                events: EventBatch::new(),
            });
        }
        let killer_offset = player_offset(usize::from(killer))?;
        let kills = read_u16(self.data, killer_offset + P_KILLS)?.saturating_add(1);
        write_u16(self.data, killer_offset + P_KILLS, kills)?;
        let total_kills = read_u32(self.data, H_TOTAL_KILLS)?.saturating_add(1);
        write_u32(self.data, H_TOTAL_KILLS, total_kills)?;
        let mut events = EventBatch::new();
        self.emit_u32(
            &mut events,
            EVENT_PLAYER_KILLED,
            killer,
            victim,
            room_id,
            total_kills,
        )?;
        let portal_vrf_nonce = if self.data[H_PORTAL_STATE] == PORTAL_LOCKED
            && total_kills >= read_u32(self.data, H_PORTAL_UNLOCK_KILLS)?
        {
            let nonce = self.next_vrf_nonce()?;
            self.data[H_PORTAL_STATE] = PORTAL_PENDING_VRF;
            write_u64(self.data, H_PORTAL_VRF_NONCE, nonce)?;
            self.emit_u64(
                &mut events,
                EVENT_PORTAL_UNLOCKED,
                killer,
                u16::MAX,
                room_id,
                nonce,
            )?;
            Some(nonce)
        } else {
            None
        };
        Ok(KillRecord {
            portal_vrf_nonce,
            events,
        })
    }

    pub fn record_death(
        &mut self,
        victim: u16,
        room_id: u16,
        event_id: u64,
    ) -> Result<EventBatch, ProgramError> {
        if !self.ensure_player_event(victim, room_id, event_id)? {
            return Ok(EventBatch::new());
        }
        let offset = player_offset(usize::from(victim))?;
        let deaths = read_u16(self.data, offset + P_DEATHS)?.saturating_add(1);
        write_u16(self.data, offset + P_DEATHS, deaths)?;
        let mut events = EventBatch::new();
        self.emit_u16(
            &mut events,
            EVENT_PLAYER_DIED,
            victim,
            u16::MAX,
            room_id,
            deaths,
        )?;
        Ok(events)
    }

    pub fn record_respawn(
        &mut self,
        player: u16,
        event_id: u64,
    ) -> Result<EventBatch, ProgramError> {
        if !self.ensure_player_event(player, u16::MAX, event_id)? {
            return Ok(EventBatch::new());
        }
        if self.data[H_RESPAWN_ENABLED] == 0 {
            return Err(LabyrinthError::InvalidPhase.into());
        }
        let offset = player_offset(usize::from(player))?;
        let respawns = read_u16(self.data, offset + P_RESPAWNS)?.saturating_add(1);
        write_u16(self.data, offset + P_RESPAWNS, respawns)?;
        let room = read_u16(self.data, offset + P_INITIAL_SPAWN_ROOM)?;
        let mut events = EventBatch::new();
        self.emit_u16(
            &mut events,
            EVENT_PLAYER_RESPAWNED,
            player,
            u16::MAX,
            room,
            room,
        )?;
        Ok(events)
    }

    pub fn resolve_portal_vrf(
        &mut self,
        randomness: [u8; 32],
        nonce: u64,
    ) -> Result<EventBatch, ProgramError> {
        if self.phase() != PHASE_ACTIVE
            || self.data[H_PORTAL_STATE] != PORTAL_PENDING_VRF
            || read_u64(self.data, H_PORTAL_VRF_NONCE)? != nonce
        {
            return Err(LabyrinthError::InvalidVrfCallback.into());
        }
        let portal_room_seed = derive(&randomness, b"portal-room");
        let portal_room = unbiased_sample(&portal_room_seed, u32::from(self.room_count())) as u16;
        self.data[H_PORTAL_SEED..H_PORTAL_SEED + 32].copy_from_slice(&randomness);
        write_u16(self.data, H_PORTAL_ROOM, portal_room)?;
        self.data[H_PORTAL_STATE] = PORTAL_SPAWNED;
        let mut events = EventBatch::new();
        let mut payload = [0u8; MAX_EVENT_PAYLOAD];
        let portal_type = read_u16(self.data, H_PORTAL_TYPE_ID)?;
        let required = read_u16(self.data, H_PORTAL_REQUIRED_PLAYERS)?;
        payload[..2].copy_from_slice(&portal_type.to_le_bytes());
        payload[2..4].copy_from_slice(&required.to_le_bytes());
        self.emit(
            &mut events,
            EVENT_PORTAL_SPAWNED,
            1,
            0,
            u16::MAX,
            u16::MAX,
            portal_room,
            &payload[..4],
        )?;
        Ok(events)
    }

    pub fn record_authority_event(
        &mut self,
        event_type: u16,
        event_version: u8,
        flags: u8,
        actor: u16,
        target: u16,
        room: u16,
        payload: &[u8],
        event_id: u64,
    ) -> Result<EventBatch, ProgramError> {
        if self.phase() != PHASE_ACTIVE
            || !is_known_event_namespace(event_type)
            || event_version == 0
            || event_version > self.data[H_EVENT_SCHEMA_VERSION]
            || payload.len() > MAX_EVENT_PAYLOAD
        {
            return Err(LabyrinthError::InvalidEvent.into());
        }
        if actor != u16::MAX && actor >= self.player_count() {
            return Err(LabyrinthError::InvalidEvent.into());
        }
        if target != u16::MAX && target >= self.player_count() {
            return Err(LabyrinthError::InvalidEvent.into());
        }
        if room != u16::MAX && room >= self.room_count() {
            return Err(LabyrinthError::InvalidEvent.into());
        }
        if !self.accept_server_event_id(event_id)? {
            return Ok(EventBatch::new());
        }
        let mut events = EventBatch::new();
        self.emit(
            &mut events,
            event_type,
            event_version,
            flags,
            actor,
            target,
            room,
            payload,
        )?;
        Ok(events)
    }

    pub fn record_match_end(
        &mut self,
        winners: &[u16],
        result_hash: [u8; 32],
        event_id: u64,
    ) -> Result<EventBatch, ProgramError> {
        if self.phase() == PHASE_TERMINAL
            && event_id <= read_u64(self.data, H_LAST_SERVER_EVENT_ID)?
        {
            return Ok(EventBatch::new());
        }
        if self.phase() != PHASE_ACTIVE || self.data[H_PORTAL_STATE] != PORTAL_SPAWNED {
            return Err(LabyrinthError::PortalNotReady.into());
        }
        self.validate_winners(winners)?;
        if !self.accept_server_event_id(event_id)? {
            return Ok(EventBatch::new());
        }
        write_u16(self.data, H_WINNER_COUNT, winners.len() as u16)?;
        write_u16(self.data, H_PRIMARY_WINNER, winners[0])?;
        self.data[H_TERMINAL_RESULT_HASH..H_TERMINAL_RESULT_HASH + 32]
            .copy_from_slice(&result_hash);
        self.data[H_WINNERS..H_WINNERS + MAX_PLAYERS * 2].fill(0xFF);
        for (index, winner) in winners.iter().enumerate() {
            write_u16(self.data, H_WINNERS + index * 2, *winner)?;
        }
        let mut events = EventBatch::new();
        let mut payload = [0u8; MAX_EVENT_PAYLOAD];
        for (index, winner) in winners.iter().enumerate() {
            payload[index * 2..index * 2 + 2].copy_from_slice(&winner.to_le_bytes());
        }
        self.emit(
            &mut events,
            EVENT_MATCH_FINISHED,
            1,
            0,
            winners[0],
            u16::MAX,
            read_u16(self.data, H_PORTAL_ROOM)?,
            &payload[..winners.len() * 2],
        )?;
        self.data[H_PHASE] = PHASE_TERMINAL;
        Ok(events)
    }

    pub fn match_seed(&self) -> [u8; 32] {
        self.data[H_MATCH_SEED..H_MATCH_SEED + 32]
            .try_into()
            .expect("fixed seed range")
    }

    pub fn portal_vrf_nonce(&self) -> Result<u64, ProgramError> {
        read_u64(self.data, H_PORTAL_VRF_NONCE)
    }

    fn next_vrf_nonce(&mut self) -> Result<u64, ProgramError> {
        let nonce = read_u64(self.data, H_NEXT_VRF_NONCE)?.saturating_add(1);
        write_u64(self.data, H_NEXT_VRF_NONCE, nonce)?;
        Ok(nonce)
    }

    fn ensure_player_event(
        &mut self,
        player: u16,
        room: u16,
        event_id: u64,
    ) -> Result<bool, ProgramError> {
        if self.phase() != PHASE_ACTIVE
            || player >= self.player_count()
            || (room != u16::MAX && room >= self.room_count())
        {
            return Err(LabyrinthError::InvalidEvent.into());
        }
        self.accept_server_event_id(event_id)
    }

    fn accept_server_event_id(&mut self, event_id: u64) -> Result<bool, ProgramError> {
        if event_id == 0 {
            return Err(LabyrinthError::InvalidEvent.into());
        }
        let current = read_u64(self.data, H_LAST_SERVER_EVENT_ID)?;
        if event_id <= current {
            return Ok(false);
        }
        write_u64(self.data, H_LAST_SERVER_EVENT_ID, event_id)?;
        Ok(true)
    }

    fn same_team(&self, left: u16, right: u16) -> Result<bool, ProgramError> {
        let left_team = read_u16(self.data, player_offset(usize::from(left))? + P_TEAM_ID)?;
        let right_team = read_u16(self.data, player_offset(usize::from(right))? + P_TEAM_ID)?;
        Ok(left_team != 0 && left_team == right_team)
    }

    fn validate_winners(&self, winners: &[u16]) -> Result<(), ProgramError> {
        if winners.len() < usize::from(read_u16(self.data, H_PORTAL_REQUIRED_PLAYERS)?)
            || winners.len() > usize::from(self.player_count())
        {
            return Err(LabyrinthError::InvalidWinnerSet.into());
        }
        let mut seen = [false; MAX_PLAYERS];
        let mut team = 0u16;
        for (position, winner) in winners.iter().enumerate() {
            let index = usize::from(*winner);
            if index >= usize::from(self.player_count()) || seen[index] {
                return Err(LabyrinthError::InvalidWinnerSet.into());
            }
            seen[index] = true;
            if self.data[H_EXTRACTION_MODE] == 1 {
                let winner_team = read_u16(self.data, player_offset(index)? + P_TEAM_ID)?;
                if winner_team == 0 || (position > 0 && winner_team != team) {
                    return Err(LabyrinthError::InvalidWinnerSet.into());
                }
                team = winner_team;
            }
        }
        Ok(())
    }

    fn choose_generation_rule(&self, random: &[u8; 32]) -> Result<GenerationRule, ProgramError> {
        let mut total = 0u32;
        for index in 0..usize::from(self.data[H_RULE_COUNT]) {
            total = total.saturating_add(u32::from(read_rule(self.data, index)?.weight));
        }
        if total == 0 {
            return Err(LabyrinthError::InvalidConfig.into());
        }
        let sample = unbiased_sample(random, total);
        let mut cursor = 0u32;
        for index in 0..usize::from(self.data[H_RULE_COUNT]) {
            let rule = read_rule(self.data, index)?;
            cursor = cursor.saturating_add(u32::from(rule.weight));
            if sample < cursor {
                return Ok(rule);
            }
        }
        Err(LabyrinthError::InvalidConfig.into())
    }

    #[allow(clippy::too_many_arguments)]
    fn emit(
        &mut self,
        events: &mut EventBatch,
        event_type: u16,
        event_version: u8,
        flags: u8,
        actor: u16,
        target: u16,
        room: u16,
        payload: &[u8],
    ) -> Result<u64, ProgramError> {
        if payload.len() > MAX_EVENT_PAYLOAD || !is_known_event_namespace(event_type) {
            return Err(LabyrinthError::InvalidEvent.into());
        }
        let sequence = read_u64(self.data, H_EVENT_SEQUENCE)?.saturating_add(1);
        let capacity = self
            .event_chunk_count()
            .saturating_mul(MAX_EVENTS_PER_CHUNK) as u64;
        if sequence > capacity {
            return Err(LabyrinthError::EventLogFull.into());
        }
        write_u64(self.data, H_EVENT_SEQUENCE, sequence)?;
        let mut encoded = [0u8; MAX_EVENT_PAYLOAD];
        encoded[..payload.len()].copy_from_slice(payload);
        events.push(MatchEvent {
            sequence,
            event_type,
            event_version,
            flags,
            actor_index: actor,
            target_index: target,
            room_id: room,
            payload_len: payload.len() as u16,
            payload: encoded,
        })?;
        Ok(sequence)
    }

    fn emit_u16(
        &mut self,
        events: &mut EventBatch,
        event_type: u16,
        actor: u16,
        target: u16,
        room: u16,
        value: u16,
    ) -> Result<u64, ProgramError> {
        self.emit(
            events,
            event_type,
            1,
            0,
            actor,
            target,
            room,
            &value.to_le_bytes(),
        )
    }

    fn emit_u32(
        &mut self,
        events: &mut EventBatch,
        event_type: u16,
        actor: u16,
        target: u16,
        room: u16,
        value: u32,
    ) -> Result<u64, ProgramError> {
        self.emit(
            events,
            event_type,
            1,
            0,
            actor,
            target,
            room,
            &value.to_le_bytes(),
        )
    }

    fn emit_u64(
        &mut self,
        events: &mut EventBatch,
        event_type: u16,
        actor: u16,
        target: u16,
        room: u16,
        value: u64,
    ) -> Result<u64, ProgramError> {
        self.emit(
            events,
            event_type,
            1,
            0,
            actor,
            target,
            room,
            &value.to_le_bytes(),
        )
    }
}

pub fn validate_match(data: &[u8]) -> Result<(), ProgramError> {
    if data.len() < MATCH_LEN
        || data[H_INITIALIZED] != 1
        || !matches!(data[H_VERSION], LEGACY_PROGRAM_VERSION | PROGRAM_VERSION)
    {
        return Err(ProgramError::UninitializedAccount);
    }
    Ok(())
}

fn validate_match_input(args: &InitializeMatchArgs<'_>) -> Result<(), ProgramError> {
    let players = usize::from(args.player_count);
    let rooms = usize::from(args.room_count);
    let ranked_record_is_zero = args.ranked_match_record == [0; 32];
    let valid_origin = match args.match_kind {
        MATCH_KIND_RANKED => !ranked_record_is_zero,
        MATCH_KIND_DIRECT => ranked_record_is_zero,
        _ => false,
    };
    if !valid_origin
        || args.config_version == 0
        || args.generation_version == 0
        || args.event_schema_version == 0
        || players == 0
        || players > MAX_PLAYERS
        || rooms < players
        || rooms > MAX_ROOMS
        || args.portal_unlock_kills == 0
        || args.portal_required_players == 0
        || args.portal_required_players > args.player_count
        || args.respawn_enabled > 1
        || args.friendly_fire_rule > 1
        || args.extraction_mode > 1
        || args.roster.len() != players * ROSTER_ENTRY_LEN
    {
        return Err(LabyrinthError::InvalidConfig.into());
    }
    for left in 0..players {
        let left_offset = left * ROSTER_ENTRY_LEN;
        let left_main = &args.roster[left_offset..left_offset + 32];
        let left_session = &args.roster[left_offset + 32..left_offset + 64];
        if left_main.iter().all(|byte| *byte == 0) || left_session.iter().all(|byte| *byte == 0) {
            return Err(LabyrinthError::InvalidRoster.into());
        }
        for right in 0..left {
            let right_offset = right * ROSTER_ENTRY_LEN;
            if left_main == &args.roster[right_offset..right_offset + 32]
                || left_session == &args.roster[right_offset + 32..right_offset + 64]
            {
                return Err(LabyrinthError::InvalidRoster.into());
            }
        }
    }
    for left in 0..usize::from(args.rule_count) {
        let rule = args.rules[left];
        if rule.type_id == 0 || rule.weight == 0 || rule.min_difficulty > rule.max_difficulty {
            return Err(LabyrinthError::InvalidConfig.into());
        }
        for right in 0..left {
            if rule.type_id == args.rules[right].type_id {
                return Err(LabyrinthError::InvalidConfig.into());
            }
        }
    }
    Ok(())
}

fn assign_unique_spawns(
    data: &mut [u8],
    player_count: usize,
    room_count: usize,
    randomness: [u8; 32],
) -> Result<(), ProgramError> {
    let mut rooms = [0u16; MAX_ROOMS];
    for (index, room) in rooms.iter_mut().take(room_count).enumerate() {
        *room = index as u16;
    }
    let mut stream = SeedStream::new(randomness);
    for index in (1..room_count).rev() {
        rooms.swap(index, stream.uniform(index + 1));
    }
    for (player, room) in rooms.iter().take(player_count).enumerate() {
        write_u16(data, player_offset(player)? + P_INITIAL_SPAWN_ROOM, *room)?;
    }
    Ok(())
}

fn difficulty_from_rule(rule: GenerationRule, random: &[u8; 32]) -> u16 {
    let min = u16::from(rule.min_difficulty);
    let range = u16::from(rule.max_difficulty)
        .saturating_sub(min)
        .saturating_add(1);
    min.saturating_add(unbiased_sample(random, u32::from(range.max(1))) as u16)
}

fn derive(seed: &[u8; 32], domain: &[u8]) -> [u8; 32] {
    hashv(&[seed, domain]).to_bytes()
}

fn unbiased_sample(randomness: &[u8; 32], upper: u32) -> u32 {
    let zone = u32::MAX - (u32::MAX % upper.max(1));
    for chunk in randomness.chunks_exact(4) {
        let value = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
        if value < zone {
            return value % upper.max(1);
        }
    }
    u32::from_le_bytes([
        randomness[28],
        randomness[29],
        randomness[30],
        randomness[31],
    ]) % upper.max(1)
}

fn is_known_event_namespace(event_type: u16) -> bool {
    matches!(
        event_type,
        EVENT_MATCH_INITIALIZED
            | EVENT_MATCH_VRF_REQUESTED
            | EVENT_MATCH_VRF_RESOLVED
            | EVENT_PLAYER_SPAWN_ASSIGNED
            | EVENT_ROOM_REVEAL_REQUESTED
            | EVENT_ROOM_RESOLVED
            | EVENT_PLAYER_KILLED
            | EVENT_PLAYER_DIED
            | EVENT_PLAYER_RESPAWNED
            | EVENT_PORTAL_UNLOCKED
            | EVENT_PORTAL_SPAWNED
            | EVENT_PORTAL_ACTIVATED
            | EVENT_PLAYER_EXTRACTED
            | EVENT_MATCH_FINISHED
    ) || (1000..=1599).contains(&event_type)
}

struct SeedStream {
    state: u64,
}

impl SeedStream {
    fn new(seed: [u8; 32]) -> Self {
        let mut state = u64::from_le_bytes(seed[..8].try_into().unwrap_or([0; 8]));
        if state == 0 {
            state = 0x9E37_79B9_7F4A_7C15;
        }
        Self { state }
    }

    fn next(&mut self) -> u64 {
        let mut value = self.state;
        value ^= value >> 12;
        value ^= value << 25;
        value ^= value >> 27;
        self.state = value;
        value.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn uniform(&mut self, upper: usize) -> usize {
        let upper = upper as u64;
        let zone = u64::MAX - (u64::MAX % upper);
        loop {
            let value = self.next();
            if value < zone {
                return (value % upper) as usize;
            }
        }
    }
}

fn player_offset(index: usize) -> Result<usize, ProgramError> {
    if index >= MAX_PLAYERS {
        return Err(LabyrinthError::InvalidRoster.into());
    }
    Ok(PLAYERS_OFFSET + index * PLAYER_ROW_LEN)
}

fn room_offset(index: usize) -> Result<usize, ProgramError> {
    if index >= MAX_ROOMS {
        return Err(LabyrinthError::InvalidRoom.into());
    }
    Ok(ROOMS_OFFSET + index * ROOM_ROW_LEN)
}

fn rule_offset(index: usize) -> Result<usize, ProgramError> {
    if index >= MAX_GENERATION_RULES {
        return Err(LabyrinthError::InvalidConfig.into());
    }
    Ok(H_RULES + index * RULE_ROW_LEN)
}

fn write_rule(data: &mut [u8], index: usize, rule: GenerationRule) -> Result<(), ProgramError> {
    let offset = rule_offset(index)?;
    write_u16(data, offset, rule.type_id)?;
    write_u16(data, offset + 2, rule.weight)?;
    data[offset + 4] = rule.min_difficulty;
    data[offset + 5] = rule.max_difficulty;
    write_u16(data, offset + 6, rule.flags)
}

fn read_rule(data: &[u8], index: usize) -> Result<GenerationRule, ProgramError> {
    let offset = rule_offset(index)?;
    Ok(GenerationRule {
        type_id: read_u16(data, offset)?,
        weight: read_u16(data, offset + 2)?,
        min_difficulty: *data
            .get(offset + 4)
            .ok_or(ProgramError::AccountDataTooSmall)?,
        max_difficulty: *data
            .get(offset + 5)
            .ok_or(ProgramError::AccountDataTooSmall)?,
        flags: read_u16(data, offset + 6)?,
    })
}

fn read_array<const N: usize>(data: &[u8], offset: usize) -> Result<[u8; N], ProgramError> {
    data.get(offset..offset + N)
        .ok_or(ProgramError::AccountDataTooSmall)?
        .try_into()
        .map_err(|_| ProgramError::InvalidAccountData)
}

fn read_u16(data: &[u8], offset: usize) -> Result<u16, ProgramError> {
    Ok(u16::from_le_bytes(read_array(data, offset)?))
}

fn read_u32(data: &[u8], offset: usize) -> Result<u32, ProgramError> {
    Ok(u32::from_le_bytes(read_array(data, offset)?))
}

fn read_u64(data: &[u8], offset: usize) -> Result<u64, ProgramError> {
    Ok(u64::from_le_bytes(read_array(data, offset)?))
}

fn write_u16(data: &mut [u8], offset: usize, value: u16) -> Result<(), ProgramError> {
    data.get_mut(offset..offset + 2)
        .ok_or(ProgramError::AccountDataTooSmall)?
        .copy_from_slice(&value.to_le_bytes());
    Ok(())
}

fn write_u32(data: &mut [u8], offset: usize, value: u32) -> Result<(), ProgramError> {
    data.get_mut(offset..offset + 4)
        .ok_or(ProgramError::AccountDataTooSmall)?
        .copy_from_slice(&value.to_le_bytes());
    Ok(())
}

fn write_u64(data: &mut [u8], offset: usize, value: u64) -> Result<(), ProgramError> {
    data.get_mut(offset..offset + 8)
        .ok_or(ProgramError::AccountDataTooSmall)?
        .copy_from_slice(&value.to_le_bytes());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::instruction::{GenerationRule, InitializeMatchArgs};

    fn roster(players: usize) -> [u8; MAX_PLAYERS * ROSTER_ENTRY_LEN] {
        let mut bytes = [0u8; MAX_PLAYERS * ROSTER_ENTRY_LEN];
        for player in 0..players {
            let offset = player * ROSTER_ENTRY_LEN;
            bytes[offset..offset + 32].fill((player + 1) as u8);
            bytes[offset + 32..offset + 64].fill((player + 31) as u8);
            bytes[offset + 64..offset + 66]
                .copy_from_slice(&(if player < 2 { 1u16 } else { 0 }).to_le_bytes());
            bytes[offset + 66..offset + 68].copy_from_slice(&0u16.to_le_bytes());
        }
        bytes
    }

    fn initialize(players: usize, rooms: usize) -> [u8; MATCH_LEN] {
        initialize_kind(players, rooms, MATCH_KIND_RANKED)
    }

    fn initialize_kind(players: usize, rooms: usize, match_kind: u8) -> [u8; MATCH_LEN] {
        let roster = roster(players);
        let args = InitializeMatchArgs {
            match_id: [1; 16],
            match_kind,
            ranked_match_record: if match_kind == MATCH_KIND_RANKED {
                [2; 32]
            } else {
                [0; 32]
            },
            config_hash: [3; 32],
            generation_profile_hash: [4; 32],
            config_version: 1,
            generation_version: 1,
            room_generation_profile: 1,
            combat_mode: 0,
            party_mode: 0,
            friendly_fire_rule: 1,
            respawn_enabled: 1,
            extraction_mode: 0,
            event_schema_version: 2,
            room_count: rooms as u16,
            player_count: players as u16,
            portal_unlock_kills: 1,
            portal_required_players: 2,
            portal_type_id: 1,
            rule_count: 3,
            event_chunk_count: 8,
            rules: [
                GenerationRule {
                    type_id: 100,
                    weight: 40,
                    min_difficulty: 1,
                    max_difficulty: 10,
                    flags: 0,
                },
                GenerationRule {
                    type_id: 250,
                    weight: 30,
                    min_difficulty: 11,
                    max_difficulty: 30,
                    flags: 1,
                },
                GenerationRule {
                    type_id: 500,
                    weight: 30,
                    min_difficulty: 31,
                    max_difficulty: 60,
                    flags: 2,
                },
                GenerationRule::default(),
                GenerationRule::default(),
                GenerationRule::default(),
                GenerationRule::default(),
                GenerationRule::default(),
                GenerationRule::default(),
                GenerationRule::default(),
                GenerationRule::default(),
                GenerationRule::default(),
                GenerationRule::default(),
                GenerationRule::default(),
                GenerationRule::default(),
                GenerationRule::default(),
            ],
            roster: &roster[..players * ROSTER_ENTRY_LEN],
        };
        let mut data = [0u8; MATCH_LEN];
        MatchState::initialize(
            &mut data,
            InitializeMatch {
                bump: 1,
                backend_authority: [9; 32],
                ranked_program: [8; 32],
                er_validator: [7; 32],
                finalizer_index: 0,
                bootstrap_permit_version: BOOTSTRAP_SCHEMA_V2,
                args,
            },
        )
        .unwrap();
        data
    }

    #[test]
    fn separate_current_state_has_stable_size() {
        assert_eq!(MATCH_HEADER_LEN, 544);
        assert_eq!(PLAYERS_OFFSET, 544);
        assert_eq!(ROOMS_OFFSET, 1_504);
        assert_eq!(MATCH_LEN, 5_088);
    }

    #[test]
    fn ranked_and_direct_origins_are_persisted_without_changing_account_size() {
        let mut ranked_data = initialize_kind(2, 4, MATCH_KIND_RANKED);
        let mut direct_data = initialize_kind(2, 4, MATCH_KIND_DIRECT);
        assert_eq!(
            MatchState::load(&mut ranked_data).unwrap().match_kind(),
            MATCH_KIND_RANKED
        );
        assert_eq!(
            MatchState::load(&mut direct_data).unwrap().match_kind(),
            MATCH_KIND_DIRECT
        );
        assert_eq!(ranked_data.len(), direct_data.len());
    }

    #[test]
    fn v4_match_accounts_remain_runtime_readable() {
        let mut data = initialize(2, 4);
        data[H_VERSION] = LEGACY_PROGRAM_VERSION;
        let mut state = MatchState::load(&mut data).unwrap();
        assert_eq!(state.version(), LEGACY_PROGRAM_VERSION);
        assert_eq!(state.request_match_vrf(0).unwrap().nonce, 1);
    }

    #[test]
    fn legacy_authority_lookup_keeps_main_wallet_compatibility() {
        let mut data = initialize(2, 4);
        let state = MatchState::load(&mut data).unwrap();
        assert_eq!(state.find_authority(&[1; 32]).unwrap(), 0);
        assert_eq!(state.find_authority(&[31; 32]).unwrap(), 0);
        assert!(state.is_finalizer_authority(&[1; 32]).unwrap());
        assert!(state.is_finalizer_authority(&[31; 32]).unwrap());
        assert!(state.find_authority(&[99; 32]).is_err());
    }

    #[test]
    fn player_paid_v5_uses_only_a_roster_session_key() {
        let mut data = initialize(2, 4);
        let state = MatchState::load(&mut data).unwrap();
        // Player 0's Main Wallet is never a v5 rent/delegation authority.
        assert!(state.find_session(&[1; 32]).is_err());
        assert_eq!(state.find_session(&[31; 32]).unwrap(), 0);
        assert_eq!(state.find_session(&[32; 32]).unwrap(), 1);
        assert!(state.find_session(&[99; 32]).is_err());
    }

    #[test]
    fn event_chunks_are_strictly_ordered_and_match_waits_for_full_mask() {
        let mut data = initialize(2, 4);
        let mut state = MatchState::load(&mut data).unwrap();
        assert!(state.mark_event_chunk_delegated(1).is_err());
        assert!(state.mark_event_chunk_delegated(0).unwrap());
        assert!(!state.mark_event_chunk_delegated(0).unwrap());
        assert!(state.require_all_event_chunks_delegated().is_err());
        for index in 1..8 {
            assert!(state.mark_event_chunk_delegated(index).unwrap());
        }
        assert_eq!(state.delegated_chunk_mask(), 0x00ff);
        assert_eq!(state.bootstrap_flags(), 0b11);
        state.require_all_event_chunks_delegated().unwrap();
    }

    #[test]
    fn chunk_zero_rules_are_bound_to_generation_profile_hash() {
        let mut data = initialize(2, 4);
        let encoded = [
            100, 0, 40, 0, 1, 10, 0, 0, 250, 0, 30, 0, 11, 30, 1, 0, 244, 1, 30, 0, 31, 60, 2, 0,
        ];
        let count = [3u8];
        let expected = hashv(&[RULES_HASH_DOMAIN, &count, &encoded]).to_bytes();
        data[H_GENERATION_PROFILE_HASH..H_GENERATION_PROFILE_HASH + 32].copy_from_slice(&expected);
        let rules = [
            GenerationRule {
                type_id: 100,
                weight: 40,
                min_difficulty: 1,
                max_difficulty: 10,
                flags: 0,
            },
            GenerationRule {
                type_id: 250,
                weight: 30,
                min_difficulty: 11,
                max_difficulty: 30,
                flags: 1,
            },
            GenerationRule {
                type_id: 500,
                weight: 30,
                min_difficulty: 31,
                max_difficulty: 60,
                flags: 2,
            },
        ];
        let mut state = MatchState::load(&mut data).unwrap();
        state.install_generation_rules(3, &rules, &encoded).unwrap();
        let mut tampered = encoded;
        tampered[3] ^= 1;
        assert!(state
            .install_generation_rules(3, &rules, &tampered)
            .is_err());
    }

    #[test]
    fn canonical_match_vrf_assigns_unique_spawns() {
        let mut data = initialize(10, 40);
        let mut state = MatchState::load(&mut data).unwrap();
        state.request_match_vrf(0).unwrap();
        let events = state.resolve_match_vrf([7; 32]).unwrap();
        assert_eq!(events.len(), 1);
        let mut assigned_events = 0;
        for first_player in (0..10).step_by(MAX_EVENTS_PER_BATCH) {
            assigned_events += state.spawn_assignment_events(first_player).unwrap().len();
        }
        assert_eq!(assigned_events, 10);
        let mut found = [false; MAX_ROOMS];
        for index in 0..10 {
            let room = usize::from(
                read_u16(
                    state.data,
                    player_offset(index).unwrap() + P_INITIAL_SPAWN_ROOM,
                )
                .unwrap(),
            );
            assert!(room < 40 && !found[room]);
            found[room] = true;
        }
    }

    #[test]
    fn room_reveal_is_first_writer_wins() {
        let mut data = initialize(2, 4);
        let mut state = MatchState::load(&mut data).unwrap();
        state.request_match_vrf(0).unwrap();
        state.resolve_match_vrf([2; 32]).unwrap();
        let first = state.enter_or_reveal_room(3, 0).unwrap();
        assert!(first.nonce.is_some());
        assert!(state.enter_or_reveal_room(3, 1).unwrap().nonce.is_none());
        state
            .resolve_room_vrf([3; 32], 3, first.nonce.unwrap())
            .unwrap();
        assert!(state.enter_or_reveal_room(3, 1).unwrap().nonce.is_none());
    }

    #[test]
    fn match_vrf_request_is_single_canonical_request() {
        let mut data = initialize(2, 4);
        let mut state = MatchState::load(&mut data).unwrap();
        let first = state.request_match_vrf(0).unwrap();
        assert_eq!(first.nonce, 1);
        assert!(state.request_match_vrf(1).is_err());
    }

    #[test]
    fn duplicate_server_kill_is_a_noop() {
        let mut data = initialize(2, 4);
        let mut state = MatchState::load(&mut data).unwrap();
        state.request_match_vrf(0).unwrap();
        state.resolve_match_vrf([2; 32]).unwrap();
        let first = state.record_kill(0, 1, 2, 41).unwrap();
        assert!(!first.events.is_empty());
        let second = state.record_kill(0, 1, 2, 41).unwrap();
        assert!(second.events.is_empty());
        assert!(second.portal_vrf_nonce.is_none());
        assert_eq!(
            read_u16(state.data, player_offset(0).unwrap() + P_KILLS).unwrap(),
            1
        );
        assert_eq!(read_u32(state.data, H_TOTAL_KILLS).unwrap(), 1);
    }

    #[test]
    fn duplicate_death_and_respawn_events_are_noops() {
        let mut data = initialize(2, 4);
        let mut state = MatchState::load(&mut data).unwrap();
        state.request_match_vrf(0).unwrap();
        state.resolve_match_vrf([2; 32]).unwrap();
        assert!(!state.record_death(1, 3, 5).unwrap().is_empty());
        assert!(state.record_death(1, 3, 5).unwrap().is_empty());
        assert!(!state.record_respawn(1, 6).unwrap().is_empty());
        assert!(state.record_respawn(1, 6).unwrap().is_empty());
        let player = player_offset(1).unwrap();
        assert_eq!(read_u16(state.data, player + P_DEATHS).unwrap(), 1);
        assert_eq!(read_u16(state.data, player + P_RESPAWNS).unwrap(), 1);
    }

    #[test]
    fn kill_automatically_starts_portal_vrf() {
        let mut data = initialize(2, 4);
        let mut state = MatchState::load(&mut data).unwrap();
        state.request_match_vrf(0).unwrap();
        state.resolve_match_vrf([2; 32]).unwrap();
        let result = state.record_kill(0, 1, 2, 1).unwrap();
        assert!(result.portal_vrf_nonce.is_some());
        assert_eq!(state.data[H_PORTAL_STATE], PORTAL_PENDING_VRF);
        state
            .resolve_portal_vrf([9; 32], result.portal_vrf_nonce.unwrap())
            .unwrap();
        assert_eq!(state.data[H_PORTAL_STATE], PORTAL_SPAWNED);
    }

    #[test]
    fn allied_players_cannot_create_a_kill_event_when_friendly_fire_is_disabled() {
        let mut data = initialize(2, 4);
        data[H_FRIENDLY_FIRE_RULE] = 0;
        let mut state = MatchState::load(&mut data).unwrap();
        state.request_match_vrf(0).unwrap();
        state.resolve_match_vrf([2; 32]).unwrap();
        assert!(state.record_kill(0, 1, 2, 1).is_err());
        assert_eq!(read_u32(state.data, H_TOTAL_KILLS).unwrap(), 0);
        assert_eq!(read_u64(state.data, H_LAST_SERVER_EVENT_ID).unwrap(), 0);
    }

    #[test]
    fn flexible_rule_ids_do_not_need_a_program_enum_change() {
        let mut data = initialize(2, 4);
        let state = MatchState::load(&mut data).unwrap();
        let selected = state.choose_generation_rule(&[0; 32]).unwrap();
        assert_eq!(selected.type_id, 100);
    }
}
