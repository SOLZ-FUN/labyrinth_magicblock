use pinocchio::{error::ProgramError, ProgramResult};
use solana_sha256_hasher::hashv;

use crate::{
    error::LabyrinthError,
    instruction::{CompactRosterProof, InitializeMatchV6Args, MAX_EVENT_PAYLOAD, ROSTER_ENTRY_LEN},
    state::{
        EVENT_MATCH_FINISHED, EVENT_MATCH_INITIALIZED, EVENT_MATCH_VRF_REQUESTED,
        EVENT_MATCH_VRF_RESOLVED, EVENT_PLAYER_DIED, EVENT_PLAYER_KILLED, EVENT_PLAYER_RESPAWNED,
        EVENT_PLAYER_SPAWN_ASSIGNED, EVENT_PORTAL_SPAWNED, EVENT_PORTAL_UNLOCKED,
        EVENT_ROOM_RESOLVED, PHASE_ACTIVE, PHASE_AWAITING_MATCH_VRF, PHASE_TERMINAL, PORTAL_LOCKED,
        PORTAL_PENDING_VRF, PORTAL_SPAWNED, VRF_PENDING, VRF_RESOLVED, VRF_UNINITIALIZED,
    },
};

pub const COMPACT_PROGRAM_VERSION: u8 = 6;
pub const COMPACT_MATCH_LEN: usize = 512;
pub const COMPACT_MAX_ROOMS: usize = 20;
pub const COMPACT_MAX_PLAYERS: usize = 12;
pub const COMPACT_UNDEFINED: u8 = u8::MAX;
pub const COMPACT_RENT_LAMPORTS: u64 = 4_454_400;

const H_INITIALIZED: usize = 0;
const H_BUMP: usize = 1;
const H_VERSION: usize = 2;
const H_PHASE: usize = 3;
const H_MATCH_KIND: usize = 4;
const H_ROOM_COUNT: usize = 5;
const H_PLAYER_COUNT: usize = 6;
const H_FINALIZER_INDEX: usize = 7;
const H_COMBAT_MODE: usize = 8;
const H_PARTY_MODE: usize = 9;
const H_FRIENDLY_FIRE: usize = 10;
const H_RESPAWN_ENABLED: usize = 11;
const H_EXTRACTION_MODE: usize = 12;
const H_EVENT_SCHEMA: usize = 13;
const H_PORTAL_REQUIRED: usize = 14;
const H_PORTAL_STATE: usize = 15;
const H_MATCH_ID: usize = 16;
const H_RANKED_MATCH_RECORD: usize = 32;
const H_ER_VALIDATOR: usize = 64;
const H_BACKEND: usize = 96;
const H_ROSTER_ROOT: usize = 128;
const H_CONFIG_HASH: usize = 160;
const H_GENERATION_ROOT: usize = 192;
const H_MATCH_SEED: usize = 224;
const H_EVENT_HASH: usize = 256;
const H_TERMINAL_HASH: usize = 288;
const H_TAKEOVER_SLOT: usize = 320;
const H_EVENT_SEQUENCE: usize = 328;
const H_NEXT_VRF_NONCE: usize = 336;
const H_MATCH_VRF_NONCE: usize = 344;
const H_PORTAL_VRF_NONCE: usize = 352;
const H_PERMIT_REVISION: usize = 360;
const H_CONFIG_VERSION: usize = 364;
const H_GENERATION_VERSION: usize = 366;
const H_GENERATION_PROFILE: usize = 368;
const H_PORTAL_UNLOCK_KILLS: usize = 370;
const H_PORTAL_TYPE_ID: usize = 372;
const H_WINNER_MASK: usize = 374;
const H_PORTAL_ROOM: usize = 376;
const H_NEXT_ROOM_SLOT: usize = 377;
const H_MATCH_VRF_STATE: usize = 378;
const H_WINNER_COUNT: usize = 379;
const H_TOTAL_KILLS: usize = 380;
const H_SPAWNS: usize = 384;
const H_KILLS: usize = 396;
const H_DEATHS: usize = 408;
const H_RESPAWNS: usize = 420;
const H_TEAMS: usize = 432;
const H_PARTIES: usize = 444;
const H_ROOM_SLOTS: usize = 456;
const H_ROOM_REVEALERS: usize = 476;
const H_LAST_SERVER_EVENT_ID: usize = 496;
const H_RESERVED: usize = 504;

const ROSTER_LEAF_DOMAIN: &[u8] = b"solz-labyrinth-roster-leaf-v1";
const ROSTER_NODE_DOMAIN: &[u8] = b"solz-labyrinth-roster-node-v1";
const EVENT_HASH_DOMAIN: &[u8] = b"solz-labyrinth-event-hash-v1";
const ROOM_SEED_DOMAIN: &[u8] = b"solz-labyrinth-room-slot-v1";
const SPAWN_DOMAIN: &[u8] = b"solz-labyrinth-spawn-v1";
const PORTAL_DOMAIN: &[u8] = b"solz-labyrinth-portal-v1";
const GENERATION_PROFILE_V1_DOMAIN: &[u8] = b"solz-labyrinth-generation-profile-v1";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CompactRoomResult {
    pub type_id: u16,
    pub difficulty: u8,
    pub enemy_seed: [u8; 32],
    pub reward_seed: [u8; 32],
    pub variant_seed: [u8; 32],
}

pub struct CompactMatchState<'a> {
    data: &'a mut [u8],
}

impl<'a> CompactMatchState<'a> {
    pub fn initialize(
        data: &'a mut [u8],
        bump: u8,
        backend_authority: [u8; 32],
        args: &InitializeMatchV6Args<'_>,
    ) -> Result<Self, ProgramError> {
        if data.len() != COMPACT_MATCH_LEN
            || args.room_count == 0
            || usize::from(args.room_count) > COMPACT_MAX_ROOMS
            || args.player_count == 0
            || usize::from(args.player_count) > COMPACT_MAX_PLAYERS
            || args.player_team_party.len() != usize::from(args.player_count) * 2
            || args.generation_profile_hash != generation_profile_v1_hash()
        {
            return Err(LabyrinthError::InvalidConfig.into());
        }
        data.fill(0);
        data[H_INITIALIZED] = 1;
        data[H_BUMP] = bump;
        data[H_VERSION] = COMPACT_PROGRAM_VERSION;
        data[H_PHASE] = PHASE_AWAITING_MATCH_VRF;
        data[H_MATCH_KIND] = args.match_kind;
        data[H_ROOM_COUNT] = args.room_count;
        data[H_PLAYER_COUNT] = args.player_count;
        data[H_FINALIZER_INDEX] = args.finalizer_index;
        data[H_COMBAT_MODE] = args.combat_mode;
        data[H_PARTY_MODE] = args.party_mode;
        data[H_FRIENDLY_FIRE] = args.friendly_fire_rule;
        data[H_RESPAWN_ENABLED] = args.respawn_enabled;
        data[H_EXTRACTION_MODE] = args.extraction_mode;
        data[H_EVENT_SCHEMA] = args.event_schema_version;
        data[H_PORTAL_REQUIRED] = args.portal_required_players;
        data[H_PORTAL_STATE] = PORTAL_LOCKED;
        data[H_MATCH_ID..H_MATCH_ID + 16].copy_from_slice(&args.match_id);
        data[H_RANKED_MATCH_RECORD..H_RANKED_MATCH_RECORD + 32]
            .copy_from_slice(&args.ranked_match_record);
        data[H_ER_VALIDATOR..H_ER_VALIDATOR + 32].copy_from_slice(&args.er_validator);
        data[H_BACKEND..H_BACKEND + 32].copy_from_slice(&backend_authority);
        data[H_ROSTER_ROOT..H_ROSTER_ROOT + 32].copy_from_slice(&args.roster_root);
        data[H_CONFIG_HASH..H_CONFIG_HASH + 32].copy_from_slice(&args.config_hash);
        data[H_GENERATION_ROOT..H_GENERATION_ROOT + 32]
            .copy_from_slice(&args.generation_profile_hash);
        write_u64(data, H_TAKEOVER_SLOT, args.takeover_slot);
        write_u32(data, H_PERMIT_REVISION, args.permit_revision);
        write_u16(data, H_CONFIG_VERSION, args.config_version);
        write_u16(data, H_GENERATION_VERSION, args.generation_version);
        write_u16(data, H_GENERATION_PROFILE, args.room_generation_profile);
        write_u16(data, H_PORTAL_UNLOCK_KILLS, args.portal_unlock_kills);
        write_u16(data, H_PORTAL_TYPE_ID, args.portal_type_id);
        data[H_PORTAL_ROOM] = COMPACT_UNDEFINED;
        data[H_SPAWNS..H_SPAWNS + COMPACT_MAX_PLAYERS].fill(COMPACT_UNDEFINED);
        data[H_ROOM_SLOTS..H_ROOM_SLOTS + COMPACT_MAX_ROOMS].fill(COMPACT_UNDEFINED);
        data[H_ROOM_REVEALERS..H_ROOM_REVEALERS + COMPACT_MAX_ROOMS].fill(COMPACT_UNDEFINED);
        data[H_RESERVED..COMPACT_MATCH_LEN].fill(0);
        for index in 0..usize::from(args.player_count) {
            data[H_TEAMS + index] = args.player_team_party[index * 2];
            data[H_PARTIES + index] = args.player_team_party[index * 2 + 1];
        }
        let mut state = Self { data };
        state.append_event(
            EVENT_MATCH_INITIALIZED,
            1,
            0,
            COMPACT_UNDEFINED,
            COMPACT_UNDEFINED,
            COMPACT_UNDEFINED,
            &[],
        )?;
        Ok(state)
    }

    pub fn load(data: &'a mut [u8]) -> Result<Self, ProgramError> {
        if data.len() != COMPACT_MATCH_LEN
            || data[H_INITIALIZED] != 1
            || data[H_VERSION] != COMPACT_PROGRAM_VERSION
        {
            return Err(ProgramError::InvalidAccountData);
        }
        Ok(Self { data })
    }

    pub fn bump(&self) -> u8 {
        self.data[H_BUMP]
    }
    pub fn phase(&self) -> u8 {
        self.data[H_PHASE]
    }
    pub fn match_id(&self) -> [u8; 16] {
        read_array::<16>(self.data, H_MATCH_ID)
    }
    pub fn ranked_match_record(&self) -> [u8; 32] {
        read_array::<32>(self.data, H_RANKED_MATCH_RECORD)
    }
    pub fn er_validator(&self) -> [u8; 32] {
        read_array::<32>(self.data, H_ER_VALIDATOR)
    }
    pub fn backend_authority(&self) -> [u8; 32] {
        read_array::<32>(self.data, H_BACKEND)
    }
    pub fn roster_root(&self) -> [u8; 32] {
        read_array::<32>(self.data, H_ROSTER_ROOT)
    }
    pub fn config_hash(&self) -> [u8; 32] {
        read_array::<32>(self.data, H_CONFIG_HASH)
    }
    pub fn generation_profile_hash(&self) -> [u8; 32] {
        read_array::<32>(self.data, H_GENERATION_ROOT)
    }
    pub fn match_seed(&self) -> [u8; 32] {
        read_array::<32>(self.data, H_MATCH_SEED)
    }
    pub fn match_vrf_nonce(&self) -> u64 {
        read_u64(self.data, H_MATCH_VRF_NONCE)
    }
    pub fn portal_vrf_nonce(&self) -> u64 {
        read_u64(self.data, H_PORTAL_VRF_NONCE)
    }
    pub fn event_hash(&self) -> [u8; 32] {
        read_array::<32>(self.data, H_EVENT_HASH)
    }
    pub fn terminal_hash(&self) -> [u8; 32] {
        read_array::<32>(self.data, H_TERMINAL_HASH)
    }
    pub fn event_sequence(&self) -> u64 {
        read_u64(self.data, H_EVENT_SEQUENCE)
    }
    pub fn room_count(&self) -> u8 {
        self.data[H_ROOM_COUNT]
    }
    pub fn player_count(&self) -> u8 {
        self.data[H_PLAYER_COUNT]
    }
    pub fn finalizer_index(&self) -> u8 {
        self.data[H_FINALIZER_INDEX]
    }
    pub fn takeover_slot(&self) -> u64 {
        read_u64(self.data, H_TAKEOVER_SLOT)
    }
    pub fn portal_state(&self) -> u8 {
        self.data[H_PORTAL_STATE]
    }
    pub fn portal_room(&self) -> u8 {
        self.data[H_PORTAL_ROOM]
    }
    pub fn winner_mask(&self) -> u16 {
        read_u16(self.data, H_WINNER_MASK)
    }
    pub fn room_slot(&self, room: u8) -> Result<Option<u8>, ProgramError> {
        if room >= self.room_count() {
            return Err(LabyrinthError::InvalidRoom.into());
        }
        let slot = self.data[H_ROOM_SLOTS + usize::from(room)];
        Ok((slot != COMPACT_UNDEFINED).then_some(slot))
    }
    pub fn spawn_room(&self, player: u8) -> Result<u8, ProgramError> {
        if player >= self.player_count() {
            return Err(LabyrinthError::InvalidRoster.into());
        }
        Ok(self.data[H_SPAWNS + usize::from(player)])
    }

    pub fn verify_member(&self, proof: &CompactRosterProof<'_>) -> Result<u8, ProgramError> {
        if proof.index >= self.player_count()
            || self.data[H_TEAMS + usize::from(proof.index)] != proof.team_id
            || self.data[H_PARTIES + usize::from(proof.index)] != proof.party_id
            || !verify_roster_proof_for_count(&self.roster_root(), proof, self.player_count())
        {
            return Err(LabyrinthError::UnauthorizedSession.into());
        }
        Ok(proof.index)
    }

    pub fn request_match_vrf(&mut self, requester: u8) -> Result<u64, ProgramError> {
        if self.phase() != PHASE_AWAITING_MATCH_VRF {
            return Err(LabyrinthError::InvalidPhase.into());
        }
        match self.data[H_MATCH_VRF_STATE] {
            VRF_UNINITIALIZED => {}
            VRF_PENDING => return Err(LabyrinthError::AlreadyRequested.into()),
            VRF_RESOLVED => return Err(LabyrinthError::RandomnessAlreadyResolved.into()),
            _ => return Err(ProgramError::InvalidAccountData),
        }
        let nonce = read_u64(self.data, H_NEXT_VRF_NONCE).saturating_add(1);
        write_u64(self.data, H_NEXT_VRF_NONCE, nonce);
        write_u64(self.data, H_MATCH_VRF_NONCE, nonce);
        self.data[H_MATCH_VRF_STATE] = VRF_PENDING;
        self.append_event(
            EVENT_MATCH_VRF_REQUESTED,
            1,
            0,
            requester,
            COMPACT_UNDEFINED,
            COMPACT_UNDEFINED,
            &nonce.to_le_bytes(),
        )?;
        Ok(nonce)
    }

    pub fn resolve_match_vrf(&mut self, randomness: [u8; 32], nonce: u64) -> ProgramResult {
        if self.data[H_MATCH_VRF_STATE] != VRF_PENDING
            || read_u64(self.data, H_MATCH_VRF_NONCE) != nonce
        {
            return Err(LabyrinthError::InvalidVrfCallback.into());
        }
        self.data[H_MATCH_SEED..H_MATCH_SEED + 32].copy_from_slice(&randomness);
        self.data[H_MATCH_VRF_STATE] = VRF_RESOLVED;
        self.data[H_PHASE] = PHASE_ACTIVE;
        self.append_event(
            EVENT_MATCH_VRF_RESOLVED,
            1,
            0,
            COMPACT_UNDEFINED,
            COMPACT_UNDEFINED,
            COMPACT_UNDEFINED,
            &randomness,
        )?;
        self.assign_unique_spawns()?;
        Ok(())
    }

    fn assign_unique_spawns(&mut self) -> ProgramResult {
        let rooms = usize::from(self.room_count());
        let players = usize::from(self.player_count());
        if players > rooms {
            return Err(LabyrinthError::InvalidConfig.into());
        }
        let seed = self.match_seed();
        let mut pool = [0u8; COMPACT_MAX_ROOMS];
        for (index, value) in pool.iter_mut().enumerate().take(rooms) {
            *value = index as u8;
        }
        for index in 0..players {
            let index_byte = [index as u8];
            let draw = hashv(&[SPAWN_DOMAIN, &seed, &index_byte]).to_bytes();
            let selected =
                index + usize::from(u16::from_le_bytes([draw[0], draw[1]])) % (rooms - index);
            pool.swap(index, selected);
            self.data[H_SPAWNS + index] = pool[index];
            self.append_event(
                EVENT_PLAYER_SPAWN_ASSIGNED,
                1,
                0,
                index as u8,
                COMPACT_UNDEFINED,
                pool[index],
                &[pool[index]],
            )?;
        }
        Ok(())
    }

    pub fn reveal_room(&mut self, room: u8, revealer: u8) -> Result<u8, ProgramError> {
        if self.phase() != PHASE_ACTIVE {
            return Err(LabyrinthError::InvalidPhase.into());
        }
        if room >= self.room_count() || revealer >= self.player_count() {
            return Err(LabyrinthError::InvalidRoom.into());
        }
        if let Some(slot) = self.room_slot(room)? {
            return Ok(slot);
        }
        let slot = self.data[H_NEXT_ROOM_SLOT];
        if slot >= self.room_count() {
            return Err(LabyrinthError::InvalidRoom.into());
        }
        self.data[H_NEXT_ROOM_SLOT] = slot + 1;
        self.data[H_ROOM_SLOTS + usize::from(room)] = slot;
        self.data[H_ROOM_REVEALERS + usize::from(room)] = revealer;
        let seed = derive_room_seed(&self.match_id(), &self.match_seed(), slot);
        let mut payload = [0u8; 34];
        payload[0] = slot;
        payload[1] = derive_room_result(seed).difficulty;
        payload[2..].copy_from_slice(&seed);
        self.append_event(
            EVENT_ROOM_RESOLVED,
            1,
            0,
            revealer,
            COMPACT_UNDEFINED,
            room,
            &payload,
        )?;
        Ok(slot)
    }

    pub fn record_kill(
        &mut self,
        killer: u8,
        victim: u8,
        room: u8,
        event_id: u64,
    ) -> Result<bool, ProgramError> {
        if !self.accept_server_event(event_id)? {
            return Ok(false);
        }
        self.require_active_player(killer)?;
        self.require_active_player(victim)?;
        if killer == victim || room >= self.room_count() {
            return Err(LabyrinthError::InvalidEvent.into());
        }
        let kills = &mut self.data[H_KILLS + usize::from(killer)];
        *kills = kills.saturating_add(1);
        write_u32(
            self.data,
            H_TOTAL_KILLS,
            read_u32(self.data, H_TOTAL_KILLS).saturating_add(1),
        );
        self.append_event(
            EVENT_PLAYER_KILLED,
            1,
            0,
            killer,
            victim,
            room,
            &event_id.to_le_bytes(),
        )?;
        let threshold = read_u16(self.data, H_PORTAL_UNLOCK_KILLS);
        if self.data[H_PORTAL_STATE] == PORTAL_LOCKED
            && read_u32(self.data, H_TOTAL_KILLS) >= u32::from(threshold)
        {
            self.append_event(
                EVENT_PORTAL_UNLOCKED,
                1,
                0,
                killer,
                COMPACT_UNDEFINED,
                room,
                &[],
            )?;
            let kills = read_u32(self.data, H_TOTAL_KILLS).to_le_bytes();
            let portal_seed =
                hashv(&[PORTAL_DOMAIN, &self.match_id(), &self.match_seed(), &kills]).to_bytes();
            let portal_room = (u16::from_le_bytes([portal_seed[0], portal_seed[1]])
                % u16::from(self.room_count())) as u8;
            self.data[H_PORTAL_ROOM] = portal_room;
            self.data[H_PORTAL_STATE] = PORTAL_SPAWNED;
            self.append_event(
                EVENT_PORTAL_SPAWNED,
                1,
                0,
                COMPACT_UNDEFINED,
                COMPACT_UNDEFINED,
                portal_room,
                &portal_seed,
            )?;
        }
        Ok(false)
    }

    pub fn request_portal_vrf(&mut self) -> Result<u64, ProgramError> {
        if self.data[H_PORTAL_STATE] != PORTAL_PENDING_VRF {
            return Err(LabyrinthError::PortalNotReady.into());
        }
        let nonce = read_u64(self.data, H_NEXT_VRF_NONCE).saturating_add(1);
        write_u64(self.data, H_NEXT_VRF_NONCE, nonce);
        write_u64(self.data, H_PORTAL_VRF_NONCE, nonce);
        Ok(nonce)
    }

    pub fn resolve_portal_vrf(&mut self, randomness: [u8; 32], nonce: u64) -> ProgramResult {
        if self.data[H_PORTAL_STATE] != PORTAL_PENDING_VRF
            || read_u64(self.data, H_PORTAL_VRF_NONCE) != nonce
        {
            return Err(LabyrinthError::InvalidVrfCallback.into());
        }
        let room =
            u16::from_le_bytes([randomness[0], randomness[1]]) % u16::from(self.room_count());
        self.data[H_PORTAL_ROOM] = room as u8;
        self.data[H_PORTAL_STATE] = PORTAL_SPAWNED;
        self.append_event(
            EVENT_PORTAL_SPAWNED,
            1,
            0,
            COMPACT_UNDEFINED,
            COMPACT_UNDEFINED,
            room as u8,
            &randomness,
        )
    }

    pub fn record_death(&mut self, player: u8, room: u8, event_id: u64) -> ProgramResult {
        if !self.accept_server_event(event_id)? {
            return Ok(());
        }
        self.require_active_player(player)?;
        self.data[H_DEATHS + usize::from(player)] =
            self.data[H_DEATHS + usize::from(player)].saturating_add(1);
        self.append_event(
            EVENT_PLAYER_DIED,
            1,
            0,
            player,
            COMPACT_UNDEFINED,
            room,
            &event_id.to_le_bytes(),
        )
    }

    pub fn record_respawn(&mut self, player: u8, event_id: u64) -> ProgramResult {
        if !self.accept_server_event(event_id)? {
            return Ok(());
        }
        self.require_active_player(player)?;
        self.data[H_RESPAWNS + usize::from(player)] =
            self.data[H_RESPAWNS + usize::from(player)].saturating_add(1);
        let room = self.spawn_room(player)?;
        self.append_event(
            EVENT_PLAYER_RESPAWNED,
            1,
            0,
            player,
            COMPACT_UNDEFINED,
            room,
            &event_id.to_le_bytes(),
        )
    }

    pub fn finish(
        &mut self,
        winner_mask: u16,
        terminal_hash: [u8; 32],
        event_id: u64,
    ) -> ProgramResult {
        if self.phase() == PHASE_TERMINAL && read_u64(self.data, H_LAST_SERVER_EVENT_ID) == event_id
        {
            return Ok(());
        }
        self.accept_server_event(event_id)?;
        if self.phase() != PHASE_ACTIVE
            || winner_mask == 0
            || winner_mask >> self.player_count() != 0
        {
            return Err(LabyrinthError::InvalidWinnerSet.into());
        }
        write_u16(self.data, H_WINNER_MASK, winner_mask);
        self.data[H_WINNER_COUNT] = winner_mask.count_ones() as u8;
        self.data[H_TERMINAL_HASH..H_TERMINAL_HASH + 32].copy_from_slice(&terminal_hash);
        self.data[H_PHASE] = PHASE_TERMINAL;
        self.append_event(
            EVENT_MATCH_FINISHED,
            1,
            0,
            COMPACT_UNDEFINED,
            COMPACT_UNDEFINED,
            self.portal_room(),
            &event_id.to_le_bytes(),
        )
    }

    pub fn append_authority_event(
        &mut self,
        event_type: u16,
        version: u8,
        flags: u8,
        actor: u8,
        target: u8,
        room: u8,
        event_id: u64,
        payload: &[u8],
    ) -> ProgramResult {
        if payload.len() > MAX_EVENT_PAYLOAD {
            return Err(LabyrinthError::InvalidEvent.into());
        }
        if !self.accept_server_event(event_id)? {
            return Ok(());
        }
        self.append_event_with_id(
            event_type, version, flags, actor, target, room, event_id, payload,
        )
    }

    fn require_active_player(&self, player: u8) -> ProgramResult {
        if self.phase() != PHASE_ACTIVE {
            return Err(LabyrinthError::InvalidPhase.into());
        }
        if player >= self.player_count() {
            return Err(LabyrinthError::InvalidRoster.into());
        }
        Ok(())
    }

    fn accept_server_event(&mut self, event_id: u64) -> Result<bool, ProgramError> {
        if event_id == 0 {
            return Err(LabyrinthError::InvalidEvent.into());
        }
        let last = read_u64(self.data, H_LAST_SERVER_EVENT_ID);
        if event_id == last {
            return Ok(false);
        }
        if event_id < last {
            return Err(LabyrinthError::EventSequenceMismatch.into());
        }
        write_u64(self.data, H_LAST_SERVER_EVENT_ID, event_id);
        Ok(true)
    }

    fn append_event(
        &mut self,
        event_type: u16,
        version: u8,
        flags: u8,
        actor: u8,
        target: u8,
        room: u8,
        payload: &[u8],
    ) -> ProgramResult {
        self.append_event_with_id(event_type, version, flags, actor, target, room, 0, payload)
    }

    fn append_event_with_id(
        &mut self,
        event_type: u16,
        version: u8,
        flags: u8,
        actor: u8,
        target: u8,
        room: u8,
        external_event_id: u64,
        payload: &[u8],
    ) -> ProgramResult {
        if payload.len() > MAX_EVENT_PAYLOAD {
            return Err(LabyrinthError::InvalidEvent.into());
        }
        let sequence = self
            .event_sequence()
            .checked_add(1)
            .ok_or(LabyrinthError::EventLogFull)?;
        let prior = self.event_hash();
        let event_type_bytes = event_type.to_le_bytes();
        let sequence_bytes = sequence.to_le_bytes();
        let external_event_id = external_event_id.to_le_bytes();
        let payload_len = (payload.len() as u16).to_le_bytes();
        let next = hashv(&[
            EVENT_HASH_DOMAIN,
            &prior,
            &sequence_bytes,
            &event_type_bytes,
            &external_event_id,
            &[version, flags, actor, target, room],
            &payload_len,
            payload,
        ])
        .to_bytes();
        self.data[H_EVENT_HASH..H_EVENT_HASH + 32].copy_from_slice(&next);
        write_u64(self.data, H_EVENT_SEQUENCE, sequence);
        Ok(())
    }
}

pub fn roster_leaf_hash(proof: &CompactRosterProof<'_>) -> [u8; 32] {
    compact_roster_leaf_hash(
        proof.index,
        &proof.main_wallet,
        &proof.session_wallet,
        proof.team_id,
        proof.party_id,
    )
}

pub fn compact_roster_leaf_hash(
    index: u8,
    main_wallet: &[u8; 32],
    session_wallet: &[u8; 32],
    team_id: u8,
    party_id: u8,
) -> [u8; 32] {
    hashv(&[
        ROSTER_LEAF_DOMAIN,
        &[index],
        main_wallet,
        session_wallet,
        &[team_id, party_id],
    ])
    .to_bytes()
}

pub fn ordered_roster_root(canonical_roster: &[u8]) -> Result<[u8; 32], ProgramError> {
    if canonical_roster.is_empty()
        || canonical_roster.len() % ROSTER_ENTRY_LEN != 0
        || canonical_roster.len() / ROSTER_ENTRY_LEN > COMPACT_MAX_PLAYERS
    {
        return Err(LabyrinthError::InvalidRoster.into());
    }
    let count = canonical_roster.len() / ROSTER_ENTRY_LEN;
    let mut nodes = [[0u8; 32]; 16];
    for (index, row) in canonical_roster.chunks_exact(ROSTER_ENTRY_LEN).enumerate() {
        if row[65] != 0 || row[67] != 0 {
            return Err(LabyrinthError::InvalidRoster.into());
        }
        nodes[index] = compact_roster_leaf_hash(
            index as u8,
            row[..32]
                .try_into()
                .map_err(|_| ProgramError::InvalidAccountData)?,
            row[32..64]
                .try_into()
                .map_err(|_| ProgramError::InvalidAccountData)?,
            row[64],
            row[66],
        );
    }
    let width = count.next_power_of_two();
    let mut level_width = width;
    while level_width > 1 {
        for index in 0..level_width / 2 {
            nodes[index] =
                hashv(&[ROSTER_NODE_DOMAIN, &nodes[index * 2], &nodes[index * 2 + 1]]).to_bytes();
        }
        level_width /= 2;
    }
    Ok(nodes[0])
}

pub fn verify_roster_proof(root: &[u8; 32], proof: &CompactRosterProof<'_>) -> bool {
    if proof.siblings.len() % 32 != 0 {
        return false;
    }
    let mut node = roster_leaf_hash(proof);
    let mut index = usize::from(proof.index);
    for sibling in proof.siblings.chunks_exact(32) {
        node = if index & 1 == 0 {
            hashv(&[ROSTER_NODE_DOMAIN, &node, sibling]).to_bytes()
        } else {
            hashv(&[ROSTER_NODE_DOMAIN, sibling, &node]).to_bytes()
        };
        index >>= 1;
    }
    node == *root
}

pub fn verify_roster_proof_for_count(
    root: &[u8; 32],
    proof: &CompactRosterProof<'_>,
    player_count: u8,
) -> bool {
    let depth = proof.siblings.len() / 32;
    player_count > 0
        && proof.index < player_count
        && depth <= 4
        && (1usize << depth) >= usize::from(player_count)
        && verify_roster_proof(root, proof)
}

pub fn validate_bootstrap_actor(
    payer: &[u8; 32],
    proof: &CompactRosterProof<'_>,
    finalizer_index: u8,
    current_slot: u64,
    takeover_slot: u64,
    expires_slot: u64,
) -> ProgramResult {
    if payer != &proof.session_wallet
        || current_slot > expires_slot
        || (current_slot < takeover_slot && proof.index != finalizer_index)
    {
        return Err(LabyrinthError::UnauthorizedSession.into());
    }
    Ok(())
}

pub fn generation_profile_v1_hash() -> [u8; 32] {
    // type id, weight, min difficulty, max difficulty for Combat, Elite,
    // Shrine, and Treasure. Boss remains intentionally excluded from v1.
    const RULES: [u8; 24] = [
        100, 0, 60, 0, 1, 60, 144, 1, 20, 0, 21, 60, 244, 1, 10, 0, 1, 20, 88, 2, 10, 0, 1, 20,
    ];
    hashv(&[GENERATION_PROFILE_V1_DOMAIN, &RULES]).to_bytes()
}

pub fn derive_room_seed(match_id: &[u8; 16], match_seed: &[u8; 32], slot: u8) -> [u8; 32] {
    hashv(&[ROOM_SEED_DOMAIN, match_id, match_seed, &[slot]]).to_bytes()
}

pub fn derive_room_result(seed: [u8; 32]) -> CompactRoomResult {
    let roll = u16::from_le_bytes([seed[0], seed[1]]) % 100;
    let type_id = match roll {
        0..=59 => 100,
        60..=79 => 400,
        80..=89 => 500,
        _ => 600,
    };
    let difficulty = 1 + seed[2] % 60;
    let enemy_seed = hashv(&[&seed, b"enemy"]).to_bytes();
    let reward_seed = hashv(&[&seed, b"reward"]).to_bytes();
    let variant_seed = hashv(&[&seed, b"variant"]).to_bytes();
    CompactRoomResult {
        type_id,
        difficulty,
        enemy_seed,
        reward_seed,
        variant_seed,
    }
}

fn read_array<const N: usize>(data: &[u8], offset: usize) -> [u8; N] {
    data[offset..offset + N].try_into().unwrap_or([0; N])
}
fn read_u16(data: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([data[offset], data[offset + 1]])
}
fn read_u32(data: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap_or([0; 4]))
}
fn read_u64(data: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(data[offset..offset + 8].try_into().unwrap_or([0; 8]))
}
fn write_u16(data: &mut [u8], offset: usize, value: u16) {
    data[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}
fn write_u32(data: &mut [u8], offset: usize, value: u32) {
    data[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}
fn write_u64(data: &mut [u8], offset: usize, value: u64) {
    data[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn proof(index: u8) -> CompactRosterProof<'static> {
        CompactRosterProof {
            index,
            main_wallet: [index + 1; 32],
            session_wallet: [index + 17; 32],
            team_id: index,
            party_id: 0,
            siblings: &[],
        }
    }

    fn args<'a>(root: [u8; 32], metadata: &'a [u8]) -> InitializeMatchV6Args<'a> {
        InitializeMatchV6Args {
            match_id: [7; 16],
            match_kind: 1,
            ranked_match_record: [0; 32],
            er_validator: [8; 32],
            roster_root: root,
            config_hash: [9; 32],
            generation_profile_hash: generation_profile_v1_hash(),
            config_version: 1,
            generation_version: 1,
            room_generation_profile: 1,
            combat_mode: 0,
            party_mode: 0,
            friendly_fire_rule: 1,
            respawn_enabled: 1,
            extraction_mode: 0,
            event_schema_version: 1,
            room_count: 20,
            player_count: 1,
            finalizer_index: 0,
            portal_unlock_kills: 1,
            portal_required_players: 1,
            portal_type_id: 1,
            permit_revision: 1,
            takeover_slot: 100,
            expires_slot: 200,
            player_team_party: metadata,
            permit_payload: &[],
            payer: proof(0),
        }
    }

    #[test]
    fn layout_and_rent_are_exact() {
        assert_eq!(COMPACT_MATCH_LEN, 512);
        assert_eq!(H_RESERVED, 504);
        assert_eq!(COMPACT_RENT_LAMPORTS, 4_454_400);
    }

    #[test]
    fn membership_room_fifo_reconstruction_and_event_replay_are_stable() {
        let member = proof(0);
        let root = roster_leaf_hash(&member);
        let mut bytes = [0u8; COMPACT_MATCH_LEN];
        let metadata = [0u8, 0];
        let init = args(root, &metadata);
        let mut state = CompactMatchState::initialize(&mut bytes, 254, [11; 32], &init).unwrap();
        assert_eq!(state.verify_member(&member).unwrap(), 0);
        let initial_hash = state.event_hash();
        let nonce = state.request_match_vrf(0).unwrap();
        state.resolve_match_vrf([42; 32], nonce).unwrap();
        let before_room = state.event_hash();
        let slot0 = state.reveal_room(10, 0).unwrap();
        let slot1 = state.reveal_room(11, 0).unwrap();
        assert_eq!((slot0, slot1), (0, 1));
        assert_eq!(state.reveal_room(10, 0).unwrap(), 0);
        assert_ne!(initial_hash, before_room);
        assert_ne!(before_room, state.event_hash());
        assert_eq!(
            derive_room_result(derive_room_seed(&[7; 16], &[42; 32], 0)),
            derive_room_result(derive_room_seed(&[7; 16], &[42; 32], 0))
        );
    }

    #[test]
    fn twenty_rooms_bind_once_and_profile_rejects_overflow() {
        let member = proof(0);
        let root = roster_leaf_hash(&member);
        let metadata = [0u8, 0];
        let mut bytes = [0u8; COMPACT_MATCH_LEN];
        let init = args(root, &metadata);
        let mut state = CompactMatchState::initialize(&mut bytes, 1, [3; 32], &init).unwrap();
        let nonce = state.request_match_vrf(0).unwrap();
        state.resolve_match_vrf([5; 32], nonce).unwrap();
        for room in 0..20 {
            assert_eq!(state.reveal_room(room, 0).unwrap(), room);
        }
        assert!(state.reveal_room(20, 0).is_err());
        let mut oversized = args(root, &metadata);
        oversized.room_count = 21;
        let mut other = [0u8; COMPACT_MATCH_LEN];
        assert!(CompactMatchState::initialize(&mut other, 1, [3; 32], &oversized).is_err());
    }

    #[test]
    fn terminal_hash_and_winner_mask_persist() {
        let member = proof(0);
        let root = roster_leaf_hash(&member);
        let metadata = [0u8, 0];
        let mut bytes = [0u8; COMPACT_MATCH_LEN];
        let init = args(root, &metadata);
        let mut state = CompactMatchState::initialize(&mut bytes, 1, [3; 32], &init).unwrap();
        let nonce = state.request_match_vrf(0).unwrap();
        state.resolve_match_vrf([5; 32], nonce).unwrap();
        state.finish(1, [99; 32], 7).unwrap();
        assert_eq!(state.phase(), PHASE_TERMINAL);
        assert_eq!(state.winner_mask(), 1);
        assert_eq!(state.terminal_hash(), [99; 32]);
    }

    #[test]
    fn session_key_only_finalizer_and_timeout_takeover_are_enforced() {
        let finalizer = proof(0);
        let takeover = proof(1);
        assert!(
            validate_bootstrap_actor(&finalizer.session_wallet, &finalizer, 0, 10, 20, 30).is_ok()
        );
        assert!(
            validate_bootstrap_actor(&finalizer.main_wallet, &finalizer, 0, 10, 20, 30).is_err()
        );
        assert!(
            validate_bootstrap_actor(&takeover.session_wallet, &takeover, 0, 19, 20, 30).is_err()
        );
        assert!(
            validate_bootstrap_actor(&takeover.session_wallet, &takeover, 0, 20, 20, 30).is_ok()
        );
        assert!(
            validate_bootstrap_actor(&takeover.session_wallet, &takeover, 0, 31, 20, 30).is_err()
        );
    }

    #[test]
    fn two_leaf_merkle_membership_rejects_mutation() {
        let left = proof(0);
        let right = proof(1);
        let left_hash = roster_leaf_hash(&left);
        let right_hash = roster_leaf_hash(&right);
        let root = hashv(&[ROSTER_NODE_DOMAIN, &left_hash, &right_hash]).to_bytes();
        let left_siblings = right_hash;
        let valid = CompactRosterProof {
            siblings: &left_siblings,
            ..left
        };
        assert!(verify_roster_proof_for_count(&root, &valid, 2));
        let changed = CompactRosterProof {
            team_id: 9,
            ..valid
        };
        assert!(!verify_roster_proof_for_count(&root, &changed, 2));

        let mut canonical = [0u8; ROSTER_ENTRY_LEN * 2];
        canonical[..32].copy_from_slice(&left.main_wallet);
        canonical[32..64].copy_from_slice(&left.session_wallet);
        canonical[64] = left.team_id;
        let right_offset = ROSTER_ENTRY_LEN;
        canonical[right_offset..right_offset + 32].copy_from_slice(&right.main_wallet);
        canonical[right_offset + 32..right_offset + 64].copy_from_slice(&right.session_wallet);
        canonical[right_offset + 64] = right.team_id;
        assert_eq!(ordered_roster_root(&canonical).unwrap(), root);
    }

    #[test]
    fn rolling_hash_replays_deterministically_and_duplicate_server_events_are_noops() {
        let member = proof(0);
        let root = roster_leaf_hash(&member);
        let metadata = [0u8, 0];
        let init = args(root, &metadata);
        let mut left_bytes = [0u8; COMPACT_MATCH_LEN];
        let mut right_bytes = [0u8; COMPACT_MATCH_LEN];
        let mut left = CompactMatchState::initialize(&mut left_bytes, 1, [3; 32], &init).unwrap();
        let mut right = CompactMatchState::initialize(&mut right_bytes, 1, [3; 32], &init).unwrap();
        for state in [&mut left, &mut right] {
            let nonce = state.request_match_vrf(0).unwrap();
            state.resolve_match_vrf([5; 32], nonce).unwrap();
            state.reveal_room(4, 0).unwrap();
            state.record_death(0, 4, 1).unwrap();
        }
        assert_eq!(left.event_hash(), right.event_hash());
        let before = left.event_hash();
        left.record_death(0, 4, 1).unwrap();
        assert_eq!(left.event_hash(), before);
    }

    #[test]
    fn portal_is_derived_from_match_root_without_another_oracle_request() {
        let member = proof(0);
        let root = roster_leaf_hash(&member);
        let metadata = [0u8, 0, 0, 0];
        let mut init = args(root, &metadata);
        init.player_count = 2;
        init.portal_unlock_kills = 1;
        let mut bytes = [0u8; COMPACT_MATCH_LEN];
        let mut state = CompactMatchState::initialize(&mut bytes, 1, [3; 32], &init).unwrap();
        let nonce = state.request_match_vrf(0).unwrap();
        state.resolve_match_vrf([5; 32], nonce).unwrap();
        assert!(!state.record_kill(0, 1, 4, 1).unwrap());
        assert_eq!(state.portal_state(), PORTAL_SPAWNED);
        assert!(state.portal_room() < 20);
    }
}
