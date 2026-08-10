use core::mem::MaybeUninit;

use ephemeral_rollups_pinocchio::{
    consts::{
        DELEGATION_PROGRAM_ID, EXTERNAL_UNDELEGATE_DISCRIMINATOR, MAGIC_CONTEXT_ID,
        MAGIC_PROGRAM_ID,
    },
    instruction::{delegate_account, undelegate},
    intent_bundle::MagicIntentBundleBuilder,
    pda::{
        delegate_buffer_pda_from_delegated_account_and_owner_program,
        delegation_metadata_pda_from_delegated_account,
        delegation_record_pda_from_delegated_account,
    },
    types::DelegateConfig,
    vrf::{
        program_identity_pda, scoped_vrf_identity, RequestRandomness, RequestRandomnessCpi,
        DEFAULT_EPHEMERAL_QUEUE, IDENTITY_SEED, VRF_PROGRAM_ID,
    },
};
use pinocchio::sysvars::Sysvar;
use pinocchio::{
    cpi::{Seed, Signer},
    error::ProgramError,
    instruction::InstructionAccount,
    AccountView, Address, ProgramResult,
};
use solana_sha256_hasher::hashv;

use crate::{
    compact::{
        validate_bootstrap_actor, verify_roster_proof_for_count, CompactMatchState,
        COMPACT_MATCH_LEN, COMPACT_UNDEFINED,
    },
    error::LabyrinthError,
    events::{
        event_chunk_pda, initialize_chunk, validate_event_chunk, validate_event_chunks,
        write_batch, write_batch_to_chunk, EVENT_CHUNK_LEN,
    },
    instruction::{
        match_end_winner_at, match_end_winner_count, parse_authority_event,
        parse_authorized_initialize_match, parse_compact_room_entry, parse_compact_roster_proof,
        parse_delegate_match_v5, parse_delegate_target, parse_initialize_event_chunk_v5,
        parse_initialize_match_v5, parse_initialize_match_v6, parse_kill, parse_match_end,
        parse_player_event, parse_portal_callback, parse_pubkey, parse_room_callback,
        parse_room_id, DELEGATE_TARGET_EVENT_CHUNK, DELEGATE_TARGET_MATCH,
        IX_COMMIT_AND_UNDELEGATE_MATCH, IX_COMMIT_MATCH, IX_DELEGATE_MATCH, IX_DELEGATE_MATCH_V5,
        IX_DELEGATE_MATCH_V6, IX_ENTER_OR_REVEAL_ROOM, IX_ENTER_OR_REVEAL_ROOM_V6,
        IX_INITIALIZE_AND_DELEGATE_EVENT_CHUNK_V5, IX_INITIALIZE_CONFIG, IX_INITIALIZE_MATCH,
        IX_INITIALIZE_MATCH_V5, IX_INITIALIZE_MATCH_V6, IX_RECORD_AUTHORITY_EVENT, IX_RECORD_DEATH,
        IX_RECORD_KILL, IX_RECORD_MATCH_END, IX_RECORD_RESPAWN, IX_REQUEST_MATCH_VRF,
        IX_REQUEST_MATCH_VRF_V6, IX_RESOLVE_MATCH_VRF, IX_RESOLVE_PORTAL_VRF, IX_RESOLVE_ROOM_VRF,
        IX_SET_BACKEND_AUTHORITY, MATCH_KIND_DIRECT, MATCH_KIND_RANKED,
    },
    pda::{CONFIG_SEED, EVENT_CHUNK_SEED, MATCH_SEED},
    session::{
        canonical_roster_hash, canonicalize_ranked_sessions,
        validate_compact_ranked_match_record_member, validate_ranked_match_record,
        validate_session_authority,
    },
    signature::{verify_prior_ed25519_instruction, verify_prior_ed25519_message},
    state::{
        ConfigState, InitializeMatch, MatchState, CONFIG_LEN, MATCH_LEN, MAX_EVENT_CHUNKS,
        MAX_PLAYERS, PHASE_TERMINAL, PROGRAM_VERSION,
    },
    system::create_pda_account,
};

const MATCH_VRF_DOMAIN: &[u8] = b"sol-zero:labyrinth:match-vrf:v1";
const ROOM_VRF_DOMAIN: &[u8] = b"sol-zero:labyrinth:room-vrf:v1";
const PORTAL_VRF_DOMAIN: &[u8] = b"sol-zero:labyrinth:portal-vrf:v1";
const VRF_DATA_BUFFER_LEN: usize = 768;
const MAGIC_DATA_BUFFER_LEN: usize = 1_280;
const LAUNCH_PERMIT_DOMAIN: &[u8] = b"solz-labyrinth-launch-permit-v2";
const BOOTSTRAP_PERMIT_VERSION: u8 = 2;
const LAUNCH_PERMIT_V6_DOMAIN: &[u8] = b"solz-labyrinth-launch-permit-v6";

fn launch_permit_message(program_id: &Address, payload: &[u8]) -> [u8; 32] {
    hashv(&[LAUNCH_PERMIT_DOMAIN, program_id.as_array(), payload]).to_bytes()
}

fn launch_permit_v6_message(program_id: &Address, payload: &[u8]) -> [u8; 32] {
    hashv(&[LAUNCH_PERMIT_V6_DOMAIN, program_id.as_array(), payload]).to_bytes()
}

#[inline(never)]
pub fn process_instruction(
    program_id: &Address,
    accounts: &[AccountView],
    data: &[u8],
) -> ProgramResult {
    if data.starts_with(&EXTERNAL_UNDELEGATE_DISCRIMINATOR) {
        return process_external_undelegate(program_id, accounts, &data[8..]);
    }
    let Some((tag, rest)) = data.split_first() else {
        return Err(ProgramError::InvalidInstructionData);
    };
    match *tag {
        IX_INITIALIZE_CONFIG => process_initialize_config(program_id, accounts, rest),
        IX_SET_BACKEND_AUTHORITY => process_set_backend_authority(program_id, accounts, rest),
        IX_INITIALIZE_MATCH => process_initialize_match(program_id, accounts, rest),
        IX_DELEGATE_MATCH => process_delegate_match(program_id, accounts, rest),
        IX_REQUEST_MATCH_VRF => process_request_match_vrf(program_id, accounts),
        IX_RESOLVE_MATCH_VRF => process_resolve_match_vrf(program_id, accounts, rest),
        IX_ENTER_OR_REVEAL_ROOM => process_enter_or_reveal_room(program_id, accounts, rest),
        IX_RESOLVE_ROOM_VRF => process_resolve_room_vrf(program_id, accounts, rest),
        IX_RECORD_KILL => process_record_kill(program_id, accounts, rest),
        IX_RECORD_DEATH => process_record_death(program_id, accounts, rest),
        IX_RECORD_RESPAWN => process_record_respawn(program_id, accounts, rest),
        IX_RESOLVE_PORTAL_VRF => process_resolve_portal_vrf(program_id, accounts, rest),
        IX_RECORD_AUTHORITY_EVENT => process_record_authority_event(program_id, accounts, rest),
        IX_RECORD_MATCH_END => process_record_match_end(program_id, accounts, rest),
        IX_COMMIT_MATCH => process_commit(program_id, accounts, rest, false),
        IX_COMMIT_AND_UNDELEGATE_MATCH => process_commit(program_id, accounts, rest, true),
        IX_INITIALIZE_MATCH_V5 => process_initialize_match_v5(program_id, accounts, rest),
        IX_INITIALIZE_AND_DELEGATE_EVENT_CHUNK_V5 => {
            process_initialize_and_delegate_event_chunk_v5(program_id, accounts, rest)
        }
        IX_DELEGATE_MATCH_V5 => process_delegate_match_v5(program_id, accounts, rest),
        IX_INITIALIZE_MATCH_V6 => process_initialize_match_v6(program_id, accounts, rest),
        IX_DELEGATE_MATCH_V6 => process_delegate_match_v6(program_id, accounts, rest),
        IX_REQUEST_MATCH_VRF_V6 => process_request_match_vrf_v6(program_id, accounts, rest),
        IX_ENTER_OR_REVEAL_ROOM_V6 => process_enter_or_reveal_room_v6(program_id, accounts, rest),
        _ => Err(ProgramError::InvalidInstructionData),
    }
}

#[inline(never)]
fn process_initialize_config(
    program_id: &Address,
    accounts: &[AccountView],
    data: &[u8],
) -> ProgramResult {
    if data.len() != 64 || accounts.len() != 4 {
        return Err(ProgramError::InvalidInstructionData);
    }
    let backend_authority = parse_pubkey(&data[..32])?;
    let ranked_program = parse_pubkey(&data[32..])?;
    if backend_authority == [0; 32] || ranked_program == [0; 32] {
        return Err(LabyrinthError::InvalidConfig.into());
    }
    let payer = &accounts[0];
    let authority = &accounts[1];
    let config_account = &accounts[2];
    let system_program = &accounts[3];
    if !payer.is_signer() || !authority.is_signer() {
        return Err(ProgramError::MissingRequiredSignature);
    }
    let (expected_config, bump) = Address::derive_program_address(&[CONFIG_SEED], program_id)
        .ok_or(ProgramError::InvalidSeeds)?;
    if config_account.address() != &expected_config {
        return Err(ProgramError::InvalidSeeds);
    }
    if config_account.data_len() != 0 {
        return Err(ProgramError::AccountAlreadyInitialized);
    }
    let bump_bytes = [bump];
    let seeds = [Seed::from(CONFIG_SEED), Seed::from(&bump_bytes)];
    create_pda_account(
        payer,
        config_account,
        system_program,
        program_id,
        CONFIG_LEN,
        &seeds,
    )?;
    ConfigState {
        bump,
        authority: *authority.address().as_array(),
        backend_authority,
        ranked_program,
    }
    .pack(&mut config_account.try_borrow_mut()?)
}

#[inline(never)]
fn process_set_backend_authority(
    program_id: &Address,
    accounts: &[AccountView],
    data: &[u8],
) -> ProgramResult {
    if accounts.len() != 2 {
        return Err(ProgramError::NotEnoughAccountKeys);
    }
    let new_backend = parse_pubkey(data)?;
    let authority = &accounts[0];
    let config_account = &accounts[1];
    if !authority.is_signer() || new_backend == [0; 32] {
        return Err(ProgramError::MissingRequiredSignature);
    }
    let mut config = load_config(program_id, config_account)?;
    if authority.address().as_array() != &config.authority {
        return Err(ProgramError::IncorrectAuthority);
    }
    config.backend_authority = new_backend;
    config.pack(&mut config_account.try_borrow_mut()?)
}

#[inline(never)]
fn process_initialize_match(
    program_id: &Address,
    accounts: &[AccountView],
    data: &[u8],
) -> ProgramResult {
    let authorized = parse_authorized_initialize_match(data)?;
    let args = authorized.args;
    let player_count = usize::from(args.player_count);
    let event_chunk_count = usize::from(args.event_chunk_count);
    let fixed_account_count = match args.match_kind {
        MATCH_KIND_RANKED => 6usize,
        MATCH_KIND_DIRECT => 5usize,
        _ => return Err(LabyrinthError::InvalidConfig.into()),
    };
    let required_accounts = fixed_account_count
        .checked_add(player_count)
        .and_then(|value| value.checked_add(event_chunk_count))
        .ok_or(ProgramError::InvalidInstructionData)?;
    if accounts.len() != required_accounts {
        return Err(ProgramError::NotEnoughAccountKeys);
    }
    let (fixed, rest) = accounts.split_at(fixed_account_count);
    let payer = &fixed[0];
    let config_account = &fixed[1];
    let (ranked_match_record, match_account, system_program) =
        if args.match_kind == MATCH_KIND_RANKED {
            (Some(&fixed[2]), &fixed[3], &fixed[4])
        } else {
            (None, &fixed[2], &fixed[3])
        };
    let instructions_sysvar = if args.match_kind == MATCH_KIND_RANKED {
        &fixed[5]
    } else {
        &fixed[4]
    };
    let (session_accounts, event_chunks) = rest.split_at(player_count);
    if !payer.is_signer() {
        return Err(ProgramError::MissingRequiredSignature);
    }
    let config = load_config(program_id, config_account)?;
    let current_slot = pinocchio::sysvars::clock::Clock::get()?.slot;
    if authorized.permit_version != BOOTSTRAP_PERMIT_VERSION
        || authorized.er_validator == [0; 32]
        || authorized.finalizer_session == [0; 32]
        || authorized.takeover_slot < current_slot
        || authorized.expires_slot <= authorized.takeover_slot
    {
        return Err(LabyrinthError::InvalidConfig.into());
    }
    if authorized.expires_slot < current_slot {
        return Err(ProgramError::InvalidInstructionData);
    }
    let permit_message = hashv(&[
        LAUNCH_PERMIT_DOMAIN,
        program_id.as_array(),
        &authorized.finalizer_session,
        authorized.permit_payload,
    ])
    .to_bytes();
    verify_prior_ed25519_instruction(
        instructions_sysvar,
        &config.backend_authority,
        &authorized.signature,
        &permit_message,
    )?;
    let ranked_program = Address::new_from_array(config.ranked_program);
    let mut canonical_roster = [0u8; MAX_PLAYERS * crate::instruction::ROSTER_ENTRY_LEN];
    let canonical_len = player_count * crate::instruction::ROSTER_ENTRY_LEN;
    canonicalize_ranked_sessions(
        &ranked_program,
        session_accounts,
        args.roster,
        &mut canonical_roster[..canonical_len],
    )?;
    let payer_session = *payer.address().as_array();
    let finalizer_index = canonical_roster[..canonical_len]
        .chunks_exact(crate::instruction::ROSTER_ENTRY_LEN)
        .position(|entry| entry[32..64] == authorized.finalizer_session)
        .ok_or(LabyrinthError::UnauthorizedSession)?;
    let payer_is_roster_session = canonical_roster[..canonical_len]
        .chunks_exact(crate::instruction::ROSTER_ENTRY_LEN)
        .any(|entry| entry[32..64] == payer_session);
    if !payer_is_roster_session {
        return Err(LabyrinthError::UnauthorizedSession.into());
    }
    // The designated finalizer starts the setup. After the signed takeover
    // slot, any immutable-roster session can safely resume the same bootstrap.
    if payer_session != authorized.finalizer_session && current_slot < authorized.takeover_slot {
        return Err(LabyrinthError::UnauthorizedSession.into());
    }
    if let Some(ranked_match_record) = ranked_match_record {
        validate_ranked_match_record(
            &ranked_program,
            ranked_match_record,
            &args.ranked_match_record,
            &args.match_id,
            &canonical_roster[..canonical_len],
            player_count,
        )?;
    } else if args.ranked_match_record != [0; 32] {
        return Err(LabyrinthError::InvalidConfig.into());
    }
    let (expected_match, bump) =
        Address::derive_program_address(&[MATCH_SEED, &args.match_id], program_id)
            .ok_or(ProgramError::InvalidSeeds)?;
    if match_account.address() != &expected_match {
        return Err(ProgramError::InvalidSeeds);
    }
    if match_account.data_len() != 0 {
        return Err(ProgramError::AccountAlreadyInitialized);
    }
    let bump_bytes = [bump];
    let match_seeds = [
        Seed::from(MATCH_SEED),
        Seed::from(args.match_id.as_slice()),
        Seed::from(&bump_bytes),
    ];
    create_pda_account(
        payer,
        match_account,
        system_program,
        program_id,
        MATCH_LEN,
        &match_seeds,
    )?;
    for (index, event_chunk) in event_chunks.iter().enumerate() {
        initialize_chunk(
            program_id,
            payer,
            event_chunk,
            system_program,
            &args.match_id,
            index as u16,
        )?;
    }
    let initial_events = {
        let canonical_args = crate::instruction::InitializeMatchArgs {
            roster: &canonical_roster[..canonical_len],
            ..args
        };
        let mut match_data = match_account.try_borrow_mut()?;
        let mut state = MatchState::initialize(
            &mut match_data,
            InitializeMatch {
                bump,
                backend_authority: config.backend_authority,
                ranked_program: config.ranked_program,
                er_validator: authorized.er_validator,
                finalizer_index: finalizer_index as u8,
                bootstrap_permit_version: authorized.permit_version,
                args: canonical_args,
            },
        )?;
        state.initialized_events()?
    };
    write_batch(
        program_id,
        &expected_match_id(match_account, program_id)?,
        event_chunk_count,
        event_chunks,
        &initial_events,
    )
}

#[inline(never)]
fn process_delegate_match(
    program_id: &Address,
    accounts: &[AccountView],
    data: &[u8],
) -> ProgramResult {
    let args = parse_delegate_target(data)?;
    if accounts.len() != 10 {
        return Err(ProgramError::NotEnoughAccountKeys);
    }
    let payer = &accounts[0];
    // The already-created Match account authorizes the listed session key.
    // It is duplicated as `target` for the match delegation case.
    let match_authority = &accounts[1];
    let owner_program = &accounts[2];
    let system_program = &accounts[3];
    let validator = &accounts[4];
    let delegation_program = &accounts[5];
    let target = &accounts[6];
    let buffer = &accounts[7];
    let delegation_record = &accounts[8];
    let delegation_metadata = &accounts[9];
    if !payer.is_signer() {
        return Err(ProgramError::MissingRequiredSignature);
    }
    if owner_program.address() != program_id {
        return Err(ProgramError::IncorrectProgramId);
    }
    if delegation_program.address() != &DELEGATION_PROGRAM_ID {
        return Err(ProgramError::IncorrectProgramId);
    }
    if !match_authority.owned_by(program_id) || match_authority.data_len() < MATCH_LEN {
        return Err(ProgramError::InvalidAccountData);
    }
    {
        let mut match_data = match_authority.try_borrow_mut()?;
        let mut state = MatchState::load(&mut match_data)?;
        assert_match_pda(program_id, match_authority, &state)?;
        if state.match_id() != args.match_id {
            return Err(ProgramError::InvalidSeeds);
        }
        state.find_session(payer.address().as_array())?;
        if state.er_validator() != *validator.address().as_array() {
            return Err(ProgramError::IncorrectAuthority);
        }
        match args.target_kind {
            DELEGATE_TARGET_EVENT_CHUNK => {
                if usize::from(args.chunk_index) >= state.event_chunk_count() {
                    return Err(LabyrinthError::InvalidConfig.into());
                }
                state.mark_event_chunk_delegated(args.chunk_index)?;
            }
            DELEGATE_TARGET_MATCH => state.require_all_event_chunks_delegated()?,
            _ => return Err(ProgramError::InvalidInstructionData),
        }
    }
    match args.target_kind {
        DELEGATE_TARGET_MATCH => {
            if target.address() != match_authority.address() {
                return Err(ProgramError::InvalidSeeds);
            }
            if !target.owned_by(program_id) || target.data_len() < MATCH_LEN {
                return Err(ProgramError::InvalidAccountData);
            }
            let bump = {
                let mut target_data = target.try_borrow_mut()?;
                let state = MatchState::load(&mut target_data)?;
                assert_match_pda(program_id, target, &state)?;
                if state.match_id() != args.match_id {
                    return Err(ProgramError::InvalidSeeds);
                }
                state.bump()
            };
            let seed_parts: [&[u8]; 2] = [MATCH_SEED, &args.match_id];
            delegate_account(
                &[
                    payer,
                    target,
                    owner_program,
                    buffer,
                    delegation_record,
                    delegation_metadata,
                    system_program,
                ],
                &seed_parts,
                bump,
                DelegateConfig {
                    commit_frequency_ms: u32::MAX,
                    validator: Some(validator.address().clone()),
                },
            )
        }
        DELEGATE_TARGET_EVENT_CHUNK => {
            validate_event_chunk(program_id, &args.match_id, args.chunk_index, target)?;
            let (_, bump) = event_chunk_pda(program_id, &args.match_id, args.chunk_index)?;
            let index_bytes = args.chunk_index.to_le_bytes();
            let seed_parts: [&[u8]; 3] = [EVENT_CHUNK_SEED, &args.match_id, &index_bytes];
            delegate_account(
                &[
                    payer,
                    target,
                    owner_program,
                    buffer,
                    delegation_record,
                    delegation_metadata,
                    system_program,
                ],
                &seed_parts,
                bump,
                DelegateConfig {
                    commit_frequency_ms: u32::MAX,
                    validator: Some(validator.address().clone()),
                },
            )
        }
        _ => return Err(ProgramError::InvalidInstructionData),
    }
}

#[inline(never)]
fn process_initialize_match_v5(
    program_id: &Address,
    accounts: &[AccountView],
    data: &[u8],
) -> ProgramResult {
    let args = parse_initialize_match_v5(data)?;
    let player_count = usize::from(args.player_count);
    let fixed_count = match args.match_kind {
        MATCH_KIND_RANKED => 6,
        MATCH_KIND_DIRECT => 5,
        _ => return Err(LabyrinthError::InvalidConfig.into()),
    };
    if accounts.len() != fixed_count + player_count {
        return Err(ProgramError::NotEnoughAccountKeys);
    }
    let payer = &accounts[0];
    let config_account = &accounts[1];
    let (ranked_record, match_account, system_program, instructions_sysvar, sessions) =
        if args.match_kind == MATCH_KIND_RANKED {
            (
                Some(&accounts[2]),
                &accounts[3],
                &accounts[4],
                &accounts[5],
                &accounts[6..],
            )
        } else {
            (
                None,
                &accounts[2],
                &accounts[3],
                &accounts[4],
                &accounts[5..],
            )
        };
    if !payer.is_signer() || !payer.is_writable() {
        return Err(ProgramError::MissingRequiredSignature);
    }
    if args.er_validator == [0; 32] {
        return Err(LabyrinthError::InvalidConfig.into());
    }
    let config = load_config(program_id, config_account)?;
    let clock = pinocchio::sysvars::clock::Clock::get()?;
    let current_unix_timestamp =
        u64::try_from(clock.unix_timestamp).map_err(|_| ProgramError::InvalidInstructionData)?;
    if args.expires_at <= current_unix_timestamp {
        return Err(ProgramError::InvalidInstructionData);
    }
    let permit_message = launch_permit_message(program_id, args.permit_payload);
    verify_prior_ed25519_message(
        instructions_sysvar,
        &config.backend_authority,
        &permit_message,
    )?;

    let ranked_program = Address::new_from_array(config.ranked_program);
    let canonical_len = player_count * crate::instruction::ROSTER_ENTRY_LEN;
    let mut canonical_roster = [0u8; MAX_PLAYERS * crate::instruction::ROSTER_ENTRY_LEN];
    canonicalize_ranked_sessions(
        &ranked_program,
        sessions,
        args.roster_metadata,
        &mut canonical_roster[..canonical_len],
    )?;
    if canonical_roster_hash(&canonical_roster[..canonical_len]) != args.roster_hash {
        return Err(LabyrinthError::InvalidRoster.into());
    }
    let initializer = usize::from(args.initializer_index) * crate::instruction::ROSTER_ENTRY_LEN;
    let payer_bytes = payer.address().as_array();
    let initializer_main = &canonical_roster[initializer..initializer + 32];
    let initializer_session = &canonical_roster[initializer + 32..initializer + 64];
    if initializer_main != payer_bytes && initializer_session != payer_bytes {
        return Err(LabyrinthError::UnauthorizedSession.into());
    }
    if let Some(ranked_record) = ranked_record {
        validate_ranked_match_record(
            &ranked_program,
            ranked_record,
            &args.ranked_match_record,
            &args.match_id,
            &canonical_roster[..canonical_len],
            player_count,
        )?;
    } else if args.ranked_match_record != [0; 32] {
        return Err(LabyrinthError::InvalidConfig.into());
    }

    let (expected_match, bump) =
        Address::derive_program_address(&[MATCH_SEED, &args.match_id], program_id)
            .ok_or(ProgramError::InvalidSeeds)?;
    if match_account.address() != &expected_match {
        return Err(ProgramError::InvalidSeeds);
    }
    if match_account.data_len() != 0 {
        return validate_existing_match_v5(
            program_id,
            match_account,
            &args,
            &canonical_roster[..canonical_len],
        );
    }
    let bump_bytes = [bump];
    let seeds = [
        Seed::from(MATCH_SEED),
        Seed::from(args.match_id.as_slice()),
        Seed::from(&bump_bytes),
    ];
    create_pda_account(
        payer,
        match_account,
        system_program,
        program_id,
        MATCH_LEN,
        &seeds,
    )?;
    let mut match_data = match_account.try_borrow_mut()?;
    MatchState::initialize_v5(
        &mut match_data,
        bump,
        config.backend_authority,
        config.ranked_program,
        args,
        &canonical_roster[..canonical_len],
    )?;
    Ok(())
}

#[inline(never)]
fn process_initialize_and_delegate_event_chunk_v5(
    program_id: &Address,
    accounts: &[AccountView],
    data: &[u8],
) -> ProgramResult {
    let args = parse_initialize_event_chunk_v5(data)?;
    if accounts.len() != 10 {
        return Err(ProgramError::NotEnoughAccountKeys);
    }
    let payer = &accounts[0];
    let match_account = &accounts[1];
    let owner_program = &accounts[2];
    let system_program = &accounts[3];
    let validator = &accounts[4];
    let delegation_program = &accounts[5];
    let chunk = &accounts[6];
    let buffer = &accounts[7];
    let delegation_record = &accounts[8];
    let delegation_metadata = &accounts[9];
    if !payer.is_signer() || !payer.is_writable() {
        return Err(ProgramError::MissingRequiredSignature);
    }
    if owner_program.address() != program_id
        || delegation_program.address() != &DELEGATION_PROGRAM_ID
    {
        return Err(ProgramError::IncorrectProgramId);
    }
    let (expected_chunk, chunk_bump) =
        event_chunk_pda(program_id, &args.match_id, args.chunk_index)?;
    if chunk.address() != &expected_chunk {
        return Err(ProgramError::InvalidSeeds);
    }
    let (fresh, initial_events) = {
        let mut match_data = match_account.try_borrow_mut()?;
        let mut state = MatchState::load(&mut match_data)?;
        assert_match_pda(program_id, match_account, &state)?;
        if state.version() != PROGRAM_VERSION || state.match_id() != args.match_id {
            return Err(ProgramError::InvalidAccountData);
        }
        state.find_authority(payer.address().as_array())?;
        if state.er_validator() != *validator.address().as_array() {
            return Err(ProgramError::IncorrectAuthority);
        }
        let bit = 1u16 << args.chunk_index;
        if state.delegated_chunk_mask() & bit != 0 {
            (false, None)
        } else {
            if args.chunk_index == 0 {
                state.install_generation_rules(
                    args.rule_count,
                    &args.rules[..usize::from(args.rule_count)],
                    args.encoded_rules,
                )?;
            }
            let events = if args.chunk_index == 0 {
                Some(state.initialized_events()?)
            } else {
                None
            };
            state.mark_event_chunk_delegated(args.chunk_index)?;
            (true, events)
        }
    };
    if !fresh {
        return validate_delegated_chunk_retry(
            program_id,
            validator,
            chunk,
            buffer,
            delegation_record,
            delegation_metadata,
        );
    }
    if chunk.data_len() != 0 {
        return Err(ProgramError::AccountAlreadyInitialized);
    }
    initialize_chunk(
        program_id,
        payer,
        chunk,
        system_program,
        &args.match_id,
        args.chunk_index,
    )?;
    if let Some(events) = initial_events.as_ref() {
        write_batch_to_chunk(program_id, &args.match_id, 0, chunk, events)?;
    }
    let index_bytes = args.chunk_index.to_le_bytes();
    let seed_parts: [&[u8]; 3] = [EVENT_CHUNK_SEED, &args.match_id, &index_bytes];
    delegate_account(
        &[
            payer,
            chunk,
            owner_program,
            buffer,
            delegation_record,
            delegation_metadata,
            system_program,
        ],
        &seed_parts,
        chunk_bump,
        DelegateConfig {
            commit_frequency_ms: u32::MAX,
            validator: Some(validator.address().clone()),
        },
    )
}

#[inline(never)]
fn process_delegate_match_v5(
    program_id: &Address,
    accounts: &[AccountView],
    data: &[u8],
) -> ProgramResult {
    let match_id = parse_delegate_match_v5(data)?;
    if accounts.len() != 9 {
        return Err(ProgramError::NotEnoughAccountKeys);
    }
    let payer = &accounts[0];
    let match_account = &accounts[1];
    let owner_program = &accounts[2];
    let system_program = &accounts[3];
    let validator = &accounts[4];
    let delegation_program = &accounts[5];
    let buffer = &accounts[6];
    let delegation_record = &accounts[7];
    let delegation_metadata = &accounts[8];
    if !payer.is_signer() || !payer.is_writable() {
        return Err(ProgramError::MissingRequiredSignature);
    }
    if owner_program.address() != program_id
        || delegation_program.address() != &DELEGATION_PROGRAM_ID
    {
        return Err(ProgramError::IncorrectProgramId);
    }
    let bump = {
        let mut match_data = match_account.try_borrow_mut()?;
        let state = MatchState::load(&mut match_data)?;
        assert_match_pda(program_id, match_account, &state)?;
        if state.version() != PROGRAM_VERSION || state.match_id() != match_id {
            return Err(ProgramError::InvalidAccountData);
        }
        state.find_authority(payer.address().as_array())?;
        if state.er_validator() != *validator.address().as_array() {
            return Err(ProgramError::IncorrectAuthority);
        }
        state.require_all_event_chunks_delegated()?;
        state.bump()
    };
    let seed_parts: [&[u8]; 2] = [MATCH_SEED, &match_id];
    delegate_account(
        &[
            payer,
            match_account,
            owner_program,
            buffer,
            delegation_record,
            delegation_metadata,
            system_program,
        ],
        &seed_parts,
        bump,
        DelegateConfig {
            commit_frequency_ms: u32::MAX,
            validator: Some(validator.address().clone()),
        },
    )
}

#[inline(never)]
fn process_initialize_match_v6(
    program_id: &Address,
    accounts: &[AccountView],
    data: &[u8],
) -> ProgramResult {
    let args = parse_initialize_match_v6(data)?;
    let fixed_count = if args.match_kind == MATCH_KIND_RANKED {
        7
    } else {
        6
    };
    if accounts.len() != fixed_count {
        return Err(ProgramError::NotEnoughAccountKeys);
    }
    let payer = &accounts[0];
    let payer_session_authority = &accounts[1];
    let config_account = &accounts[2];
    let (ranked_record, match_account, system_program, instructions_sysvar) =
        if args.match_kind == MATCH_KIND_RANKED {
            (Some(&accounts[3]), &accounts[4], &accounts[5], &accounts[6])
        } else {
            (None, &accounts[3], &accounts[4], &accounts[5])
        };
    if !payer.is_signer() || !payer.is_writable() {
        return Err(ProgramError::MissingRequiredSignature);
    }
    let clock = pinocchio::sysvars::clock::Clock::get()?;
    validate_bootstrap_actor(
        payer.address().as_array(),
        &args.payer,
        args.finalizer_index,
        clock.slot,
        args.takeover_slot,
        args.expires_slot,
    )?;
    if !verify_roster_proof_for_count(&args.roster_root, &args.payer, args.player_count)
        || args.player_team_party[usize::from(args.payer.index) * 2] != args.payer.team_id
        || args.player_team_party[usize::from(args.payer.index) * 2 + 1] != args.payer.party_id
    {
        return Err(LabyrinthError::InvalidRoster.into());
    }
    let config = load_config(program_id, config_account)?;
    let ranked_program = Address::new_from_array(config.ranked_program);
    validate_session_authority(
        &ranked_program,
        payer_session_authority,
        &args.payer.main_wallet,
        &args.payer.session_wallet,
    )?;
    if let Some(record) = ranked_record {
        validate_compact_ranked_match_record_member(
            &ranked_program,
            record,
            &args.ranked_match_record,
            &args.match_id,
            usize::from(args.player_count),
            usize::from(args.payer.index),
            &args.payer.main_wallet,
        )?;
    } else if args.ranked_match_record != [0; 32] {
        return Err(LabyrinthError::InvalidConfig.into());
    }
    let permit_message = launch_permit_v6_message(program_id, args.permit_payload);
    verify_prior_ed25519_message(
        instructions_sysvar,
        &config.backend_authority,
        &permit_message,
    )?;
    let (expected_match, bump) =
        Address::derive_program_address(&[MATCH_SEED, &args.match_id], program_id)
            .ok_or(ProgramError::InvalidSeeds)?;
    if match_account.address() != &expected_match {
        return Err(ProgramError::InvalidSeeds);
    }
    if match_account.data_len() != 0 {
        if !match_account.owned_by(program_id) || match_account.data_len() != COMPACT_MATCH_LEN {
            return Err(ProgramError::AccountAlreadyInitialized);
        }
        let mut existing_data = match_account.try_borrow_mut()?;
        let existing = CompactMatchState::load(&mut existing_data)?;
        assert_compact_match_pda(program_id, match_account, &existing)?;
        if existing.match_id() != args.match_id
            || existing.ranked_match_record() != args.ranked_match_record
            || existing.er_validator() != args.er_validator
            || existing.roster_root() != args.roster_root
            || existing.config_hash() != args.config_hash
            || existing.generation_profile_hash() != args.generation_profile_hash
            || existing.finalizer_index() != args.finalizer_index
            || existing.takeover_slot() != args.takeover_slot
        {
            return Err(ProgramError::AccountAlreadyInitialized);
        }
        return existing.verify_member(&args.payer).map(|_| ());
    }
    let bump_bytes = [bump];
    let seeds = [
        Seed::from(MATCH_SEED),
        Seed::from(args.match_id.as_slice()),
        Seed::from(&bump_bytes),
    ];
    create_pda_account(
        payer,
        match_account,
        system_program,
        program_id,
        COMPACT_MATCH_LEN,
        &seeds,
    )?;
    let mut match_data = match_account.try_borrow_mut()?;
    CompactMatchState::initialize(&mut match_data, bump, config.backend_authority, &args)?;
    Ok(())
}

#[inline(never)]
fn process_delegate_match_v6(
    program_id: &Address,
    accounts: &[AccountView],
    data: &[u8],
) -> ProgramResult {
    if accounts.len() != 11 || data.len() < 16 {
        return Err(ProgramError::NotEnoughAccountKeys);
    }
    let match_id: [u8; 16] = data[..16]
        .try_into()
        .map_err(|_| ProgramError::InvalidInstructionData)?;
    let proof = parse_compact_roster_proof(&data[16..])?;
    let payer = &accounts[0];
    let session_authority = &accounts[1];
    let config_account = &accounts[2];
    let match_account = &accounts[3];
    let owner_program = &accounts[4];
    let system_program = &accounts[5];
    let validator = &accounts[6];
    let delegation_program = &accounts[7];
    let buffer = &accounts[8];
    let delegation_record = &accounts[9];
    let delegation_metadata = &accounts[10];
    if !payer.is_signer()
        || !payer.is_writable()
        || payer.address().as_array() != &proof.session_wallet
    {
        return Err(LabyrinthError::UnauthorizedSession.into());
    }
    if owner_program.address() != program_id
        || delegation_program.address() != &DELEGATION_PROGRAM_ID
    {
        return Err(ProgramError::IncorrectProgramId);
    }
    if match_account.owned_by(&DELEGATION_PROGRAM_ID) {
        return validate_delegated_match_retry(
            program_id,
            validator,
            match_account,
            buffer,
            delegation_record,
            delegation_metadata,
            COMPACT_MATCH_LEN,
        );
    }
    let config = load_config(program_id, config_account)?;
    let ranked_program = Address::new_from_array(config.ranked_program);
    let bump = {
        let mut match_data = match_account.try_borrow_mut()?;
        let state = CompactMatchState::load(&mut match_data)?;
        assert_compact_match_pda(program_id, match_account, &state)?;
        if state.match_id() != match_id || state.er_validator() != *validator.address().as_array() {
            return Err(ProgramError::InvalidSeeds);
        }
        state.verify_member(&proof)?;
        state.bump()
    };
    validate_session_authority(
        &ranked_program,
        session_authority,
        &proof.main_wallet,
        &proof.session_wallet,
    )?;
    let seed_parts: [&[u8]; 2] = [MATCH_SEED, &match_id];
    delegate_account(
        &[
            payer,
            match_account,
            owner_program,
            buffer,
            delegation_record,
            delegation_metadata,
            system_program,
        ],
        &seed_parts,
        bump,
        DelegateConfig {
            commit_frequency_ms: u32::MAX,
            validator: Some(validator.address().clone()),
        },
    )
}

#[inline(never)]
fn process_request_match_vrf_v6(
    program_id: &Address,
    accounts: &[AccountView],
    data: &[u8],
) -> ProgramResult {
    if accounts.len() != 9 {
        return Err(ProgramError::NotEnoughAccountKeys);
    }
    let proof = parse_compact_roster_proof(data)?;
    let payer = &accounts[0];
    let session_authority = &accounts[1];
    let config = load_config(program_id, &accounts[2])?;
    let ranked_program = Address::new_from_array(config.ranked_program);
    let match_account = &accounts[3];
    assert_vrf_request_accounts(program_id, payer, &accounts[4], &accounts[5], &accounts[8])?;
    if payer.address().as_array() != &proof.session_wallet {
        return Err(LabyrinthError::UnauthorizedSession.into());
    }
    let (match_id, nonce) = {
        let mut match_data = match_account.try_borrow_mut()?;
        let mut state = CompactMatchState::load(&mut match_data)?;
        assert_compact_match_pda(program_id, match_account, &state)?;
        let requester = state.verify_member(&proof)?;
        let nonce = state.request_match_vrf(requester)?;
        (state.match_id(), nonce)
    };
    validate_session_authority(
        &ranked_program,
        session_authority,
        &proof.main_wallet,
        &proof.session_wallet,
    )?;
    let nonce_bytes = nonce.to_le_bytes();
    let caller_seed = hashv(&[MATCH_VRF_DOMAIN, &match_id, &nonce_bytes]).to_bytes();
    request_vrf(
        program_id,
        payer,
        match_account,
        &accounts[4],
        &accounts[5],
        &accounts[6],
        &accounts[7],
        &accounts[8],
        &[],
        caller_seed,
        &[IX_RESOLVE_MATCH_VRF],
        &[],
    )
}

#[inline(never)]
fn process_enter_or_reveal_room_v6(
    program_id: &Address,
    accounts: &[AccountView],
    data: &[u8],
) -> ProgramResult {
    if accounts.len() != 4 {
        return Err(ProgramError::NotEnoughAccountKeys);
    }
    let (room, proof) = parse_compact_room_entry(data)?;
    let payer = &accounts[0];
    let session_authority = &accounts[1];
    let config = load_config(program_id, &accounts[2])?;
    let ranked_program = Address::new_from_array(config.ranked_program);
    let match_account = &accounts[3];
    if !payer.is_signer() || payer.address().as_array() != &proof.session_wallet {
        return Err(LabyrinthError::UnauthorizedSession.into());
    }
    validate_session_authority(
        &ranked_program,
        session_authority,
        &proof.main_wallet,
        &proof.session_wallet,
    )?;
    let mut match_data = match_account.try_borrow_mut()?;
    let mut state = CompactMatchState::load(&mut match_data)?;
    assert_compact_match_pda(program_id, match_account, &state)?;
    let revealer = state.verify_member(&proof)?;
    state.reveal_room(room, revealer).map(|_| ())
}

#[inline(never)]
fn process_request_match_vrf(program_id: &Address, accounts: &[AccountView]) -> ProgramResult {
    let (header, chunks) = split_vrf_and_chunks(accounts)?;
    let payer = &header[0];
    let match_account = &header[1];
    assert_vrf_request_accounts(program_id, payer, &header[2], &header[3], &header[6])?;
    let (match_id, nonce, events, chunk_count) = {
        let mut data = match_account.try_borrow_mut()?;
        let mut state = MatchState::load(&mut data)?;
        assert_match_pda(program_id, match_account, &state)?;
        let requester = state.find_session(payer.address().as_array())?;
        let request = state.request_match_vrf(requester)?;
        (
            state.match_id(),
            request.nonce,
            request.events,
            state.event_chunk_count(),
        )
    };
    write_batch(program_id, &match_id, chunk_count, chunks, &events)?;
    let nonce_bytes = nonce.to_le_bytes();
    let caller_seed = hashv(&[MATCH_VRF_DOMAIN, &match_id, &nonce_bytes]).to_bytes();
    request_vrf(
        program_id,
        &header[0],
        &header[1],
        &header[2],
        &header[3],
        &header[4],
        &header[5],
        &header[6],
        chunks,
        caller_seed,
        &[IX_RESOLVE_MATCH_VRF],
        &[],
    )
}

#[inline(never)]
fn process_resolve_match_vrf(
    program_id: &Address,
    accounts: &[AccountView],
    data: &[u8],
) -> ProgramResult {
    if data.len() != 32 || accounts.len() < 2 {
        return Err(ProgramError::InvalidInstructionData);
    }
    assert_scoped_vrf_callback(program_id, &accounts[0])?;
    let randomness: [u8; 32] = data
        .try_into()
        .map_err(|_| ProgramError::InvalidInstructionData)?;
    let match_account = &accounts[1];
    if match_account.data_len() == COMPACT_MATCH_LEN {
        if accounts.len() != 2 {
            return Err(ProgramError::NotEnoughAccountKeys);
        }
        let mut match_data = match_account.try_borrow_mut()?;
        let mut state = CompactMatchState::load(&mut match_data)?;
        assert_compact_match_pda(program_id, match_account, &state)?;
        let nonce = state.match_vrf_nonce();
        return state.resolve_match_vrf(randomness, nonce);
    }
    let chunks = &accounts[2..];
    let mut match_data = match_account.try_borrow_mut()?;
    let mut state = MatchState::load(&mut match_data)?;
    assert_match_pda(program_id, match_account, &state)?;
    let match_id = state.match_id();
    let chunk_count = state.event_chunk_count();
    let player_count = usize::from(state.player_count());
    let events = state.resolve_match_vrf(randomness)?;
    write_batch(program_id, &match_id, chunk_count, chunks, &events)?;
    for first_player in (0..player_count).step_by(crate::events::MAX_EVENTS_PER_BATCH) {
        let events = state.spawn_assignment_events(first_player)?;
        write_batch(program_id, &match_id, chunk_count, chunks, &events)?;
    }
    Ok(())
}

#[inline(never)]
fn process_enter_or_reveal_room(
    program_id: &Address,
    accounts: &[AccountView],
    data: &[u8],
) -> ProgramResult {
    let room_id = parse_room_id(data)?;
    let (header, chunks) = split_vrf_and_chunks(accounts)?;
    let payer = &header[0];
    let match_account = &header[1];
    assert_vrf_request_accounts(program_id, payer, &header[2], &header[3], &header[6])?;
    let (match_id, match_seed, nonce, events, chunk_count) = {
        let mut match_data = match_account.try_borrow_mut()?;
        let mut state = MatchState::load(&mut match_data)?;
        assert_match_pda(program_id, match_account, &state)?;
        let requester = state.find_session(payer.address().as_array())?;
        let request = state.enter_or_reveal_room(room_id, requester)?;
        (
            state.match_id(),
            state.match_seed(),
            request.nonce,
            request.events,
            state.event_chunk_count(),
        )
    };
    write_batch(program_id, &match_id, chunk_count, chunks, &events)?;
    let Some(nonce) = nonce else {
        return Ok(());
    };
    let nonce_bytes = nonce.to_le_bytes();
    let room_bytes = room_id.to_le_bytes();
    let caller_seed = hashv(&[
        ROOM_VRF_DOMAIN,
        &match_id,
        &match_seed,
        &room_bytes,
        &nonce_bytes,
    ])
    .to_bytes();
    let mut callback_args = [0u8; 10];
    callback_args[..2].copy_from_slice(&room_bytes);
    callback_args[2..].copy_from_slice(&nonce_bytes);
    request_vrf(
        program_id,
        &header[0],
        &header[1],
        &header[2],
        &header[3],
        &header[4],
        &header[5],
        &header[6],
        chunks,
        caller_seed,
        &[IX_RESOLVE_ROOM_VRF],
        &callback_args,
    )
}

#[inline(never)]
fn process_resolve_room_vrf(
    program_id: &Address,
    accounts: &[AccountView],
    data: &[u8],
) -> ProgramResult {
    let (randomness, room_id, nonce) = parse_room_callback(data)?;
    if accounts.len() < 2 {
        return Err(ProgramError::NotEnoughAccountKeys);
    }
    assert_scoped_vrf_callback(program_id, &accounts[0])?;
    let match_account = &accounts[1];
    let (match_id, events, chunk_count) = {
        let mut match_data = match_account.try_borrow_mut()?;
        let mut state = MatchState::load(&mut match_data)?;
        assert_match_pda(program_id, match_account, &state)?;
        let events = state.resolve_room_vrf(randomness, room_id, nonce)?;
        (state.match_id(), events, state.event_chunk_count())
    };
    write_batch(program_id, &match_id, chunk_count, &accounts[2..], &events)
}

#[inline(never)]
fn process_record_kill(
    program_id: &Address,
    accounts: &[AccountView],
    data: &[u8],
) -> ProgramResult {
    let (killer, victim, room_id, event_id) = parse_kill(data)?;
    if accounts.len() == 2 && accounts[1].data_len() == COMPACT_MATCH_LEN {
        let backend = &accounts[0];
        let match_account = &accounts[1];
        let mut match_data = match_account.try_borrow_mut()?;
        let mut state = CompactMatchState::load(&mut match_data)?;
        assert_compact_backend_and_match(program_id, backend, match_account, &state)?;
        state.record_kill(
            u8::try_from(killer).map_err(|_| LabyrinthError::InvalidRoster)?,
            u8::try_from(victim).map_err(|_| LabyrinthError::InvalidRoster)?,
            u8::try_from(room_id).map_err(|_| LabyrinthError::InvalidRoom)?,
            event_id,
        )?;
        return Ok(());
    }
    let (header, chunks) = split_vrf_and_chunks(accounts)?;
    let backend = &header[0];
    let match_account = &header[1];
    assert_vrf_request_accounts(program_id, backend, &header[2], &header[3], &header[6])?;
    let (match_id, match_seed, portal_nonce, events, chunk_count) = {
        let mut match_data = match_account.try_borrow_mut()?;
        let mut state = MatchState::load(&mut match_data)?;
        assert_backend_and_match(program_id, backend, match_account, &state)?;
        let record = state.record_kill(killer, victim, room_id, event_id)?;
        (
            state.match_id(),
            state.match_seed(),
            record.portal_vrf_nonce,
            record.events,
            state.event_chunk_count(),
        )
    };
    write_batch(program_id, &match_id, chunk_count, chunks, &events)?;
    let Some(nonce) = portal_nonce else {
        return Ok(());
    };
    let nonce_bytes = nonce.to_le_bytes();
    let caller_seed = hashv(&[PORTAL_VRF_DOMAIN, &match_id, &match_seed, &nonce_bytes]).to_bytes();
    request_vrf(
        program_id,
        &header[0],
        &header[1],
        &header[2],
        &header[3],
        &header[4],
        &header[5],
        &header[6],
        chunks,
        caller_seed,
        &[IX_RESOLVE_PORTAL_VRF],
        &nonce_bytes,
    )
}

#[inline(never)]
fn process_record_death(
    program_id: &Address,
    accounts: &[AccountView],
    data: &[u8],
) -> ProgramResult {
    let (victim, room_id, event_id) = parse_player_event(data)?;
    let (backend, match_account, chunks) = split_backend_event_accounts(accounts)?;
    if match_account.data_len() == COMPACT_MATCH_LEN {
        if !chunks.is_empty() {
            return Err(ProgramError::InvalidInstructionData);
        }
        let mut match_data = match_account.try_borrow_mut()?;
        let mut state = CompactMatchState::load(&mut match_data)?;
        assert_compact_backend_and_match(program_id, backend, match_account, &state)?;
        return state.record_death(
            u8::try_from(victim).map_err(|_| LabyrinthError::InvalidRoster)?,
            u8::try_from(room_id).map_err(|_| LabyrinthError::InvalidRoom)?,
            event_id,
        );
    }
    let (match_id, events, chunk_count) = {
        let mut match_data = match_account.try_borrow_mut()?;
        let mut state = MatchState::load(&mut match_data)?;
        assert_backend_and_match(program_id, backend, match_account, &state)?;
        let events = state.record_death(victim, room_id, event_id)?;
        (state.match_id(), events, state.event_chunk_count())
    };
    write_batch(program_id, &match_id, chunk_count, chunks, &events)
}

#[inline(never)]
fn process_record_respawn(
    program_id: &Address,
    accounts: &[AccountView],
    data: &[u8],
) -> ProgramResult {
    let (player, _, event_id) = parse_player_event(data)?;
    let (backend, match_account, chunks) = split_backend_event_accounts(accounts)?;
    if match_account.data_len() == COMPACT_MATCH_LEN {
        if !chunks.is_empty() {
            return Err(ProgramError::InvalidInstructionData);
        }
        let mut match_data = match_account.try_borrow_mut()?;
        let mut state = CompactMatchState::load(&mut match_data)?;
        assert_compact_backend_and_match(program_id, backend, match_account, &state)?;
        return state.record_respawn(
            u8::try_from(player).map_err(|_| LabyrinthError::InvalidRoster)?,
            event_id,
        );
    }
    let (match_id, events, chunk_count) = {
        let mut match_data = match_account.try_borrow_mut()?;
        let mut state = MatchState::load(&mut match_data)?;
        assert_backend_and_match(program_id, backend, match_account, &state)?;
        let events = state.record_respawn(player, event_id)?;
        (state.match_id(), events, state.event_chunk_count())
    };
    write_batch(program_id, &match_id, chunk_count, chunks, &events)
}

#[inline(never)]
fn process_resolve_portal_vrf(
    program_id: &Address,
    accounts: &[AccountView],
    data: &[u8],
) -> ProgramResult {
    let (randomness, nonce) = parse_portal_callback(data)?;
    if accounts.len() < 2 {
        return Err(ProgramError::NotEnoughAccountKeys);
    }
    assert_scoped_vrf_callback(program_id, &accounts[0])?;
    let match_account = &accounts[1];
    if match_account.data_len() == COMPACT_MATCH_LEN {
        if accounts.len() != 2 {
            return Err(ProgramError::NotEnoughAccountKeys);
        }
        let mut match_data = match_account.try_borrow_mut()?;
        let mut state = CompactMatchState::load(&mut match_data)?;
        assert_compact_match_pda(program_id, match_account, &state)?;
        return state.resolve_portal_vrf(randomness, nonce);
    }
    let (match_id, events, chunk_count) = {
        let mut match_data = match_account.try_borrow_mut()?;
        let mut state = MatchState::load(&mut match_data)?;
        assert_match_pda(program_id, match_account, &state)?;
        let events = state.resolve_portal_vrf(randomness, nonce)?;
        (state.match_id(), events, state.event_chunk_count())
    };
    write_batch(program_id, &match_id, chunk_count, &accounts[2..], &events)
}

#[inline(never)]
fn process_record_authority_event(
    program_id: &Address,
    accounts: &[AccountView],
    data: &[u8],
) -> ProgramResult {
    let args = parse_authority_event(data)?;
    let (backend, match_account, chunks) = split_backend_event_accounts(accounts)?;
    if match_account.data_len() == COMPACT_MATCH_LEN {
        if !chunks.is_empty() {
            return Err(ProgramError::InvalidInstructionData);
        }
        let mut match_data = match_account.try_borrow_mut()?;
        let mut state = CompactMatchState::load(&mut match_data)?;
        assert_compact_backend_and_match(program_id, backend, match_account, &state)?;
        return state.append_authority_event(
            args.event_type,
            args.event_version,
            args.flags,
            u8::try_from(args.actor_index).unwrap_or(COMPACT_UNDEFINED),
            u8::try_from(args.target_index).unwrap_or(COMPACT_UNDEFINED),
            u8::try_from(args.room_id).unwrap_or(COMPACT_UNDEFINED),
            args.event_id,
            args.payload,
        );
    }
    let (match_id, events, chunk_count) = {
        let mut match_data = match_account.try_borrow_mut()?;
        let mut state = MatchState::load(&mut match_data)?;
        assert_backend_and_match(program_id, backend, match_account, &state)?;
        let events = state.record_authority_event(
            args.event_type,
            args.event_version,
            args.flags,
            args.actor_index,
            args.target_index,
            args.room_id,
            args.payload,
            args.event_id,
        )?;
        (state.match_id(), events, state.event_chunk_count())
    };
    write_batch(program_id, &match_id, chunk_count, chunks, &events)
}

#[inline(never)]
fn process_record_match_end(
    program_id: &Address,
    accounts: &[AccountView],
    data: &[u8],
) -> ProgramResult {
    let args = parse_match_end(data)?;
    let winner_count = match_end_winner_count(&args);
    let mut winners = [0u16; MAX_PLAYERS];
    for (index, winner) in winners.iter_mut().take(winner_count).enumerate() {
        *winner = match_end_winner_at(&args, index)?;
    }
    let (backend, match_account, chunks) = split_backend_event_accounts(accounts)?;
    if match_account.data_len() == COMPACT_MATCH_LEN {
        if !chunks.is_empty() {
            return Err(ProgramError::InvalidInstructionData);
        }
        let mut winner_mask = 0u16;
        for winner in winners.iter().take(winner_count) {
            if usize::from(*winner) >= MAX_PLAYERS {
                return Err(LabyrinthError::InvalidWinnerSet.into());
            }
            winner_mask |= 1u16 << *winner;
        }
        let mut match_data = match_account.try_borrow_mut()?;
        let mut state = CompactMatchState::load(&mut match_data)?;
        assert_compact_backend_and_match(program_id, backend, match_account, &state)?;
        return state.finish(winner_mask, args.result_hash, args.event_id);
    }
    let (match_id, events, chunk_count) = {
        let mut match_data = match_account.try_borrow_mut()?;
        let mut state = MatchState::load(&mut match_data)?;
        assert_backend_and_match(program_id, backend, match_account, &state)?;
        let events =
            state.record_match_end(&winners[..winner_count], args.result_hash, args.event_id)?;
        (state.match_id(), events, state.event_chunk_count())
    };
    write_batch(program_id, &match_id, chunk_count, chunks, &events)
}

#[inline(never)]
fn process_commit(
    program_id: &Address,
    accounts: &[AccountView],
    data: &[u8],
    undelegate_after: bool,
) -> ProgramResult {
    if accounts.len() == 6 && accounts[3].data_len() == COMPACT_MATCH_LEN {
        let proof = parse_compact_roster_proof(data)?;
        let payer = &accounts[0];
        let session_authority = &accounts[1];
        let config = load_config(program_id, &accounts[2])?;
        let match_account = &accounts[3];
        let magic_context = &accounts[4];
        let magic_program = &accounts[5];
        if !payer.is_signer() || payer.address().as_array() != &proof.session_wallet {
            return Err(LabyrinthError::UnauthorizedSession.into());
        }
        validate_session_authority(
            &Address::new_from_array(config.ranked_program),
            session_authority,
            &proof.main_wallet,
            &proof.session_wallet,
        )?;
        {
            let mut match_data = match_account.try_borrow_mut()?;
            let state = CompactMatchState::load(&mut match_data)?;
            assert_compact_match_pda(program_id, match_account, &state)?;
            let player = state.verify_member(&proof)?;
            if state.phase() != PHASE_TERMINAL || state.winner_mask() & (1u16 << player) == 0 {
                return Err(LabyrinthError::MatchNotTerminal.into());
            }
        }
        if magic_context.address() != &MAGIC_CONTEXT_ID
            || magic_program.address() != &MAGIC_PROGRAM_ID
        {
            return Err(ProgramError::IncorrectProgramId);
        }
        let mut buffer = [0u8; MAGIC_DATA_BUFFER_LEN];
        let builder = MagicIntentBundleBuilder::new(
            payer.clone(),
            magic_context.clone(),
            magic_program.clone(),
        );
        return if undelegate_after {
            builder
                .commit_and_undelegate(core::slice::from_ref(match_account))
                .build_and_invoke(&mut buffer)
        } else {
            builder
                .commit(core::slice::from_ref(match_account))
                .build_and_invoke(&mut buffer)
        };
    }
    if !data.is_empty() {
        return Err(ProgramError::InvalidInstructionData);
    }
    if accounts.len() < 5 {
        return Err(ProgramError::NotEnoughAccountKeys);
    }
    let backend = &accounts[0];
    let match_account = &accounts[1];
    let chunk_count = {
        let mut match_data = match_account.try_borrow_mut()?;
        let state = MatchState::load(&mut match_data)?;
        assert_backend_and_match(program_id, backend, match_account, &state)?;
        if undelegate_after && state.phase() != PHASE_TERMINAL {
            return Err(LabyrinthError::MatchNotTerminal.into());
        }
        state.event_chunk_count()
    };
    let magic_offset = 2 + chunk_count;
    if accounts.len() != magic_offset + 2 {
        return Err(ProgramError::NotEnoughAccountKeys);
    }
    let chunks = &accounts[2..magic_offset];
    let magic_context = &accounts[magic_offset];
    let magic_program = &accounts[magic_offset + 1];
    if magic_context.address() != &MAGIC_CONTEXT_ID || magic_program.address() != &MAGIC_PROGRAM_ID
    {
        return Err(ProgramError::IncorrectProgramId);
    }
    let match_id = expected_match_id(match_account, program_id)?;
    validate_event_chunks(program_id, &match_id, chunk_count, chunks)?;
    let committed = &accounts[1..magic_offset];
    let mut buffer = [0u8; MAGIC_DATA_BUFFER_LEN];
    let builder = MagicIntentBundleBuilder::new(
        backend.clone(),
        magic_context.clone(),
        magic_program.clone(),
    );
    if undelegate_after {
        builder
            .commit_and_undelegate(committed)
            .build_and_invoke(&mut buffer)
    } else {
        builder.commit(committed).build_and_invoke(&mut buffer)
    }
}

#[inline(never)]
fn process_external_undelegate(
    program_id: &Address,
    accounts: &[AccountView],
    callback_args: &[u8],
) -> ProgramResult {
    if accounts.len() < 3 {
        return Err(ProgramError::NotEnoughAccountKeys);
    }
    undelegate(
        &accounts[0],
        program_id,
        &accounts[1],
        &accounts[2],
        callback_args,
    )
}

#[allow(clippy::too_many_arguments)]
#[inline(never)]
fn request_vrf(
    program_id: &Address,
    payer: &AccountView,
    match_account: &AccountView,
    program_identity: &AccountView,
    oracle_queue: &AccountView,
    system_program: &AccountView,
    slot_hashes: &AccountView,
    vrf_program: &AccountView,
    event_chunks: &[AccountView],
    caller_seed: [u8; 32],
    callback_discriminator: &[u8],
    callback_args: &[u8],
) -> ProgramResult {
    if event_chunks.len() > MAX_EVENT_CHUNKS {
        return Err(ProgramError::InvalidInstructionData);
    }
    const UNINIT: MaybeUninit<InstructionAccount> = MaybeUninit::uninit();
    let mut metas = [UNINIT; MAX_EVENT_CHUNKS + 1];
    metas[0].write(InstructionAccount::writable(match_account.address()));
    for (index, event_chunk) in event_chunks.iter().enumerate() {
        metas[index + 1].write(InstructionAccount::writable(event_chunk.address()));
    }
    let callback_accounts = unsafe {
        core::slice::from_raw_parts(
            metas.as_ptr() as *const InstructionAccount,
            event_chunks.len() + 1,
        )
    };
    let request = RequestRandomness {
        high_priority: false,
        caller_seed,
        callback_program_id: program_id,
        callback_discriminator,
        callback_accounts_metas: callback_accounts,
        callback_args,
    };
    let cpi = RequestRandomnessCpi {
        payer,
        program_identity,
        oracle_queue,
        system_program,
        slot_hashes,
        vrf_program,
        request,
    };
    if cpi.serialized_size() > VRF_DATA_BUFFER_LEN {
        return Err(ProgramError::InvalidInstructionData);
    }
    let (_, bump) = program_identity_pda(program_id);
    let bump_bytes = [bump];
    let identity_seeds = [Seed::from(IDENTITY_SEED), Seed::from(&bump_bytes)];
    let signer = Signer::from(&identity_seeds);
    let mut data = [0u8; VRF_DATA_BUFFER_LEN];
    cpi.invoke_signed(&mut data, &[signer])
}

fn split_vrf_and_chunks(
    accounts: &[AccountView],
) -> Result<(&[AccountView], &[AccountView]), ProgramError> {
    if accounts.len() < 7 {
        return Err(ProgramError::NotEnoughAccountKeys);
    }
    Ok(accounts.split_at(7))
}

fn split_backend_event_accounts(
    accounts: &[AccountView],
) -> Result<(&AccountView, &AccountView, &[AccountView]), ProgramError> {
    if accounts.len() < 2 {
        return Err(ProgramError::NotEnoughAccountKeys);
    }
    let (fixed, chunks) = accounts.split_at(2);
    Ok((&fixed[0], &fixed[1], chunks))
}

fn assert_vrf_request_accounts(
    program_id: &Address,
    payer: &AccountView,
    program_identity: &AccountView,
    oracle_queue: &AccountView,
    vrf_program: &AccountView,
) -> ProgramResult {
    if !payer.is_signer() || !payer.is_writable() {
        return Err(ProgramError::MissingRequiredSignature);
    }
    let (expected_identity, _) = program_identity_pda(program_id);
    if program_identity.address() != &expected_identity {
        return Err(ProgramError::InvalidSeeds);
    }
    if oracle_queue.address() != &DEFAULT_EPHEMERAL_QUEUE
        || vrf_program.address() != &VRF_PROGRAM_ID
    {
        return Err(ProgramError::IncorrectProgramId);
    }
    Ok(())
}

fn assert_scoped_vrf_callback(program_id: &Address, identity: &AccountView) -> ProgramResult {
    let (expected, _) = scoped_vrf_identity(program_id);
    if !identity.is_signer() || identity.address() != &expected {
        return Err(LabyrinthError::InvalidVrfCallback.into());
    }
    Ok(())
}

fn assert_backend_and_match(
    program_id: &Address,
    backend: &AccountView,
    match_account: &AccountView,
    state: &MatchState<'_>,
) -> ProgramResult {
    if !backend.is_signer() || backend.address().as_array() != &state.backend_authority() {
        return Err(ProgramError::IncorrectAuthority);
    }
    assert_match_pda(program_id, match_account, state)
}

fn assert_match_pda(
    program_id: &Address,
    match_account: &AccountView,
    state: &MatchState<'_>,
) -> ProgramResult {
    let match_id = state.match_id();
    let expected =
        Address::derive_address(&[MATCH_SEED, &match_id], Some(state.bump()), program_id);
    if match_account.address() != &expected {
        return Err(ProgramError::InvalidSeeds);
    }
    Ok(())
}

fn assert_compact_match_pda(
    program_id: &Address,
    match_account: &AccountView,
    state: &CompactMatchState<'_>,
) -> ProgramResult {
    let match_id = state.match_id();
    let expected =
        Address::derive_address(&[MATCH_SEED, &match_id], Some(state.bump()), program_id);
    if match_account.address() != &expected {
        return Err(ProgramError::InvalidSeeds);
    }
    Ok(())
}

fn assert_compact_backend_and_match(
    program_id: &Address,
    backend: &AccountView,
    match_account: &AccountView,
    state: &CompactMatchState<'_>,
) -> ProgramResult {
    if !backend.is_signer() || backend.address().as_array() != &state.backend_authority() {
        return Err(ProgramError::IncorrectAuthority);
    }
    assert_compact_match_pda(program_id, match_account, state)
}

fn expected_match_id(
    match_account: &AccountView,
    program_id: &Address,
) -> Result<[u8; 16], ProgramError> {
    let mut data = match_account.try_borrow_mut()?;
    let state = MatchState::load(&mut data)?;
    assert_match_pda(program_id, match_account, &state)?;
    Ok(state.match_id())
}

fn validate_existing_match_v5(
    program_id: &Address,
    match_account: &AccountView,
    args: &crate::instruction::InitializeMatchV5Args<'_>,
    canonical_roster: &[u8],
) -> ProgramResult {
    if !match_account.owned_by(program_id) || match_account.data_len() < MATCH_LEN {
        return Err(ProgramError::AccountAlreadyInitialized);
    }
    let mut data = match_account.try_borrow_mut()?;
    let state = MatchState::load(&mut data)?;
    assert_match_pda(program_id, match_account, &state)?;
    if !state.matches_v5_initialization(args, canonical_roster)? {
        return Err(ProgramError::AccountAlreadyInitialized);
    }
    Ok(())
}

fn validate_delegated_chunk_retry(
    program_id: &Address,
    validator: &AccountView,
    chunk: &AccountView,
    buffer: &AccountView,
    delegation_record: &AccountView,
    delegation_metadata: &AccountView,
) -> ProgramResult {
    if !chunk.owned_by(&DELEGATION_PROGRAM_ID) || chunk.data_len() != EVENT_CHUNK_LEN {
        return Err(ProgramError::InvalidAccountData);
    }
    if buffer.address()
        != &delegate_buffer_pda_from_delegated_account_and_owner_program(
            chunk.address(),
            program_id,
        )
        || delegation_record.address()
            != &delegation_record_pda_from_delegated_account(chunk.address())
        || delegation_metadata.address()
            != &delegation_metadata_pda_from_delegated_account(chunk.address())
        || !delegation_record.owned_by(&DELEGATION_PROGRAM_ID)
        || !delegation_metadata.owned_by(&DELEGATION_PROGRAM_ID)
    {
        return Err(ProgramError::InvalidSeeds);
    }
    let record = delegation_record.try_borrow()?;
    let metadata = delegation_metadata.try_borrow()?;
    if record.len() < 72
        || metadata.len() < 8
        || record[..8] != 100u64.to_le_bytes()
        || metadata[..8] != 102u64.to_le_bytes()
        || record[8..40] != *validator.address().as_array()
        || record[40..72] != *program_id.as_array()
    {
        return Err(ProgramError::InvalidAccountData);
    }
    Ok(())
}

fn validate_delegated_match_retry(
    program_id: &Address,
    validator: &AccountView,
    match_account: &AccountView,
    buffer: &AccountView,
    delegation_record: &AccountView,
    delegation_metadata: &AccountView,
    expected_len: usize,
) -> ProgramResult {
    if !match_account.owned_by(&DELEGATION_PROGRAM_ID) || match_account.data_len() != expected_len {
        return Err(ProgramError::InvalidAccountData);
    }
    if buffer.address()
        != &delegate_buffer_pda_from_delegated_account_and_owner_program(
            match_account.address(),
            program_id,
        )
        || delegation_record.address()
            != &delegation_record_pda_from_delegated_account(match_account.address())
        || delegation_metadata.address()
            != &delegation_metadata_pda_from_delegated_account(match_account.address())
        || !delegation_record.owned_by(&DELEGATION_PROGRAM_ID)
        || !delegation_metadata.owned_by(&DELEGATION_PROGRAM_ID)
    {
        return Err(ProgramError::InvalidSeeds);
    }
    let record = delegation_record.try_borrow()?;
    let metadata = delegation_metadata.try_borrow()?;
    if record.len() < 72
        || metadata.len() < 8
        || record[..8] != 100u64.to_le_bytes()
        || metadata[..8] != 102u64.to_le_bytes()
        || record[8..40] != *validator.address().as_array()
        || record[40..72] != *program_id.as_array()
    {
        return Err(ProgramError::InvalidAccountData);
    }
    Ok(())
}

fn load_config(
    program_id: &Address,
    config_account: &AccountView,
) -> Result<ConfigState, ProgramError> {
    if !config_account.owned_by(program_id) || config_account.data_len() < CONFIG_LEN {
        return Err(ProgramError::UninitializedAccount);
    }
    let config = ConfigState::unpack(&config_account.try_borrow()?)?;
    let expected = Address::derive_address(&[CONFIG_SEED], Some(config.bump), program_id);
    if config_account.address() != &expected {
        return Err(ProgramError::InvalidSeeds);
    }
    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permit_digest_binds_program_and_every_payload_byte() {
        let program = Address::new_from_array([7; 32]);
        let other_program = Address::new_from_array([8; 32]);
        let payload = [9u8; 217 + 12 * 4];
        let expected = launch_permit_message(&program, &payload);
        let mut tampered = payload;
        tampered[0] ^= 1;
        assert_ne!(expected, launch_permit_message(&program, &tampered));
        assert_ne!(expected, launch_permit_message(&other_program, &payload));
    }

    #[test]
    fn v6_permit_digest_is_domain_separated_and_mutation_bound() {
        let program = Address::new_from_array([7; 32]);
        let payload = [11u8; 217 + 12 * 2];
        let expected = launch_permit_v6_message(&program, &payload);
        let mut changed = payload;
        changed[216] ^= 1; // expiry/finalizer/config prefix is signed.
        assert_ne!(expected, launch_permit_v6_message(&program, &changed));
        assert_ne!(expected, launch_permit_message(&program, &payload));
    }
}
