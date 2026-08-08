use pinocchio::{error::ProgramError, AccountView, Address, ProgramResult};

use crate::{
    error::LabyrinthError,
    instruction::MAX_EVENT_PAYLOAD,
    pda::EVENT_CHUNK_SEED,
    state::{MAX_EVENTS_PER_CHUNK, MAX_EVENT_CHUNKS, PROGRAM_VERSION},
    system::create_pda_account,
};

pub const EVENT_CHUNK_HEADER_LEN: usize = 32;
pub const EVENT_RECORD_LEN: usize = 88;
pub const EVENT_CHUNK_LEN: usize = EVENT_CHUNK_HEADER_LEN + MAX_EVENTS_PER_CHUNK * EVENT_RECORD_LEN;
/// Keep transient on-stack batches intentionally small for the 4 KiB sBPF
/// frame limit. Match-start spawn events are emitted in consecutive batches.
pub const MAX_EVENTS_PER_BATCH: usize = 2;

const C_INITIALIZED: usize = 0;
const C_BUMP: usize = 1;
const C_VERSION: usize = 2;
const C_MATCH_ID: usize = 4;
const C_INDEX: usize = 20;
const C_EVENT_COUNT: usize = 22;
const C_FIRST_SEQUENCE: usize = 24;

#[derive(Clone, Copy)]
pub struct MatchEvent {
    pub sequence: u64,
    pub event_type: u16,
    pub event_version: u8,
    pub flags: u8,
    pub actor_index: u16,
    pub target_index: u16,
    pub room_id: u16,
    pub payload_len: u16,
    pub payload: [u8; MAX_EVENT_PAYLOAD],
}

impl Default for MatchEvent {
    fn default() -> Self {
        Self {
            sequence: 0,
            event_type: 0,
            event_version: 0,
            flags: 0,
            actor_index: u16::MAX,
            target_index: u16::MAX,
            room_id: u16::MAX,
            payload_len: 0,
            payload: [0; MAX_EVENT_PAYLOAD],
        }
    }
}

#[derive(Clone, Copy)]
pub struct EventBatch {
    events: [MatchEvent; MAX_EVENTS_PER_BATCH],
    count: usize,
}

impl EventBatch {
    pub const fn new() -> Self {
        Self {
            events: [MatchEvent {
                sequence: 0,
                event_type: 0,
                event_version: 0,
                flags: 0,
                actor_index: u16::MAX,
                target_index: u16::MAX,
                room_id: u16::MAX,
                payload_len: 0,
                payload: [0; MAX_EVENT_PAYLOAD],
            }; MAX_EVENTS_PER_BATCH],
            count: 0,
        }
    }

    pub fn push(&mut self, event: MatchEvent) -> Result<(), ProgramError> {
        if self.count >= self.events.len() {
            return Err(LabyrinthError::EventBatchFull.into());
        }
        self.events[self.count] = event;
        self.count += 1;
        Ok(())
    }

    pub fn iter(&self) -> core::slice::Iter<'_, MatchEvent> {
        self.events[..self.count].iter()
    }

    pub const fn is_empty(&self) -> bool {
        self.count == 0
    }

    pub const fn len(&self) -> usize {
        self.count
    }
}

pub fn event_chunk_pda(
    program_id: &Address,
    match_id: &[u8; 16],
    index: u16,
) -> Result<(Address, u8), ProgramError> {
    let index_bytes = index.to_le_bytes();
    Address::derive_program_address(&[EVENT_CHUNK_SEED, match_id, &index_bytes], program_id)
        .ok_or(ProgramError::InvalidSeeds)
}

pub fn initialize_chunk(
    program_id: &Address,
    payer: &AccountView,
    chunk: &AccountView,
    system_program: &AccountView,
    match_id: &[u8; 16],
    index: u16,
) -> ProgramResult {
    let (expected, bump) = event_chunk_pda(program_id, match_id, index)?;
    if chunk.address() != &expected {
        return Err(ProgramError::InvalidSeeds);
    }
    if chunk.data_len() != 0 {
        return Err(ProgramError::AccountAlreadyInitialized);
    }
    let index_bytes = index.to_le_bytes();
    let bump_bytes = [bump];
    let seeds = [
        pinocchio::cpi::Seed::from(EVENT_CHUNK_SEED),
        pinocchio::cpi::Seed::from(match_id.as_slice()),
        pinocchio::cpi::Seed::from(&index_bytes),
        pinocchio::cpi::Seed::from(&bump_bytes),
    ];
    create_pda_account(
        payer,
        chunk,
        system_program,
        program_id,
        EVENT_CHUNK_LEN,
        &seeds,
    )?;
    let mut data = chunk.try_borrow_mut()?;
    data[..EVENT_CHUNK_LEN].fill(0);
    data[C_INITIALIZED] = 1;
    data[C_BUMP] = bump;
    data[C_VERSION] = PROGRAM_VERSION;
    data[C_MATCH_ID..C_MATCH_ID + 16].copy_from_slice(match_id);
    write_u16(&mut data, C_INDEX, index)?;
    Ok(())
}

pub fn validate_event_chunks(
    program_id: &Address,
    match_id: &[u8; 16],
    event_chunk_count: usize,
    chunks: &[AccountView],
) -> ProgramResult {
    if event_chunk_count == 0
        || event_chunk_count > MAX_EVENT_CHUNKS
        || chunks.len() != event_chunk_count
    {
        return Err(ProgramError::NotEnoughAccountKeys);
    }
    for (index, chunk) in chunks.iter().enumerate() {
        let (expected, _) = event_chunk_pda(program_id, match_id, index as u16)?;
        if chunk.address() != &expected
            || !chunk.owned_by(program_id)
            || chunk.data_len() < EVENT_CHUNK_LEN
        {
            return Err(ProgramError::InvalidAccountData);
        }
        let data = chunk.try_borrow()?;
        if data[C_INITIALIZED] != 1
            || data[C_VERSION] != PROGRAM_VERSION
            || data[C_MATCH_ID..C_MATCH_ID + 16] != *match_id
            || read_u16(&data, C_INDEX)? != index as u16
        {
            return Err(ProgramError::InvalidAccountData);
        }
    }
    Ok(())
}

pub fn write_batch(
    program_id: &Address,
    match_id: &[u8; 16],
    event_chunk_count: usize,
    chunks: &[AccountView],
    batch: &EventBatch,
) -> ProgramResult {
    if batch.is_empty() {
        return Ok(());
    }
    validate_event_chunks(program_id, match_id, event_chunk_count, chunks)?;
    for event in batch.iter() {
        let zero_index = event
            .sequence
            .checked_sub(1)
            .ok_or(LabyrinthError::InvalidEvent)? as usize;
        let chunk_index = zero_index / MAX_EVENTS_PER_CHUNK;
        let slot = zero_index % MAX_EVENTS_PER_CHUNK;
        if chunk_index >= event_chunk_count {
            return Err(LabyrinthError::EventLogFull.into());
        }
        let chunk = &chunks[chunk_index];
        let mut data = chunk.try_borrow_mut()?;
        let event_count = usize::from(read_u16(&data, C_EVENT_COUNT)?);
        if event_count != slot {
            return Err(LabyrinthError::EventSequenceMismatch.into());
        }
        if slot == 0 {
            write_u64(&mut data, C_FIRST_SEQUENCE, event.sequence)?;
        }
        let offset = EVENT_CHUNK_HEADER_LEN + slot * EVENT_RECORD_LEN;
        data[offset..offset + EVENT_RECORD_LEN].fill(0);
        write_u64(&mut data, offset, event.sequence)?;
        write_u16(&mut data, offset + 8, event.event_type)?;
        data[offset + 10] = event.event_version;
        data[offset + 11] = event.flags;
        write_u16(&mut data, offset + 12, event.actor_index)?;
        write_u16(&mut data, offset + 14, event.target_index)?;
        write_u16(&mut data, offset + 16, event.room_id)?;
        write_u16(&mut data, offset + 18, event.payload_len)?;
        let payload_len = usize::from(event.payload_len);
        if payload_len > MAX_EVENT_PAYLOAD {
            return Err(LabyrinthError::InvalidEvent.into());
        }
        data[offset + 20..offset + 20 + payload_len].copy_from_slice(&event.payload[..payload_len]);
        write_u16(&mut data, C_EVENT_COUNT, (slot + 1) as u16)?;
    }
    Ok(())
}

fn read_u16(data: &[u8], offset: usize) -> Result<u16, ProgramError> {
    let value: [u8; 2] = data
        .get(offset..offset + 2)
        .ok_or(ProgramError::AccountDataTooSmall)?
        .try_into()
        .map_err(|_| ProgramError::InvalidAccountData)?;
    Ok(u16::from_le_bytes(value))
}

fn write_u16(data: &mut [u8], offset: usize, value: u16) -> ProgramResult {
    data.get_mut(offset..offset + 2)
        .ok_or(ProgramError::AccountDataTooSmall)?
        .copy_from_slice(&value.to_le_bytes());
    Ok(())
}

fn write_u64(data: &mut [u8], offset: usize, value: u64) -> ProgramResult {
    data.get_mut(offset..offset + 8)
        .ok_or(ProgramError::AccountDataTooSmall)?
        .copy_from_slice(&value.to_le_bytes());
    Ok(())
}
