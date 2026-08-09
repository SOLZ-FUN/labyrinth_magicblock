use pinocchio::{error::ProgramError, AccountView, Address, ProgramResult};
use solana_sha256_hasher::hashv;

use crate::{
    error::LabyrinthError,
    instruction::{INITIALIZE_ROSTER_ENTRY_LEN, ROSTER_ENTRY_LEN},
};

const SESSION_AUTHORITY_SEED: &[u8] = b"session_authority";
const MATCH_RECORD_SEED: &[u8] = b"match_record";
const SESSION_AUTHORITY_LEN: usize = 76;
const MATCH_RECORD_ROSTER_OFFSET: usize = 260;
const MATCH_RECORD_MIN_LEN: usize = MATCH_RECORD_ROSTER_OFFSET + 12 * 32;
const ROSTER_HASH_DOMAIN: &[u8] = b"solz-labyrinth-roster-v1";

pub fn canonical_roster_hash(canonical_roster: &[u8]) -> [u8; 32] {
    hashv(&[ROSTER_HASH_DOMAIN, canonical_roster]).to_bytes()
}

pub fn validate_ranked_match_record(
    ranked_program: &Address,
    match_record: &AccountView,
    expected_address: &[u8; 32],
    match_id: &[u8; 16],
    roster: &[u8],
    player_count: usize,
) -> ProgramResult {
    if match_record.address().as_array() != expected_address
        || !match_record.owned_by(ranked_program)
        || match_record.data_len() < MATCH_RECORD_MIN_LEN
    {
        return Err(LabyrinthError::InvalidRoster.into());
    }
    let (expected_pda, _) =
        Address::derive_program_address(&[MATCH_RECORD_SEED, match_id], ranked_program)
            .ok_or(ProgramError::InvalidSeeds)?;
    if match_record.address() != &expected_pda {
        return Err(ProgramError::InvalidSeeds);
    }
    let data = match_record.try_borrow()?;
    if data[0] != 1
        || data[2..18] != *match_id
        || data[144] != 1
        || usize::from(u16::from_le_bytes([data[140], data[141]])) != player_count
    {
        return Err(LabyrinthError::InvalidRoster.into());
    }
    for index in 0..player_count {
        let roster_offset = index * ROSTER_ENTRY_LEN;
        let record_offset = MATCH_RECORD_ROSTER_OFFSET + index * 32;
        if roster[roster_offset..roster_offset + 32] != data[record_offset..record_offset + 32] {
            return Err(LabyrinthError::InvalidRoster.into());
        }
    }
    Ok(())
}

pub fn canonicalize_ranked_sessions(
    ranked_program: &Address,
    session_accounts: &[AccountView],
    roster_metadata: &[u8],
    canonical_roster: &mut [u8],
) -> ProgramResult {
    if roster_metadata.len() != session_accounts.len() * INITIALIZE_ROSTER_ENTRY_LEN
        || canonical_roster.len() != session_accounts.len() * ROSTER_ENTRY_LEN
    {
        return Err(LabyrinthError::InvalidRoster.into());
    }
    for (index, session_authority) in session_accounts.iter().enumerate() {
        let offset = index * ROSTER_ENTRY_LEN;
        if !session_authority.owned_by(ranked_program)
            || session_authority.data_len() < SESSION_AUTHORITY_LEN
        {
            return Err(LabyrinthError::UnauthorizedSession.into());
        }
        let data = session_authority.try_borrow()?;
        let main_wallet: &[u8; 32] = data[2..34]
            .try_into()
            .map_err(|_| ProgramError::InvalidAccountData)?;
        let session_wallet = &data[34..66];
        let (expected_pda, _) =
            Address::derive_program_address(&[SESSION_AUTHORITY_SEED, main_wallet], ranked_program)
                .ok_or(ProgramError::InvalidSeeds)?;
        if session_authority.address() != &expected_pda {
            return Err(LabyrinthError::UnauthorizedSession.into());
        }
        if data[0] != 1
            || data[66] != 0
            || data[67] != 1
            || main_wallet.iter().all(|byte| *byte == 0)
            || session_wallet.iter().all(|byte| *byte == 0)
        {
            return Err(LabyrinthError::UnauthorizedSession.into());
        }
        canonical_roster[offset..offset + 32].copy_from_slice(main_wallet);
        canonical_roster[offset + 32..offset + 64].copy_from_slice(session_wallet);
        let metadata_offset = index * INITIALIZE_ROSTER_ENTRY_LEN;
        canonical_roster[offset + 64..offset + 68]
            .copy_from_slice(&roster_metadata[metadata_offset..metadata_offset + 4]);
    }
    Ok(())
}
