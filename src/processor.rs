use core::mem::MaybeUninit;

use ephemeral_rollups_pinocchio::{
    consts::{EXTERNAL_UNDELEGATE_DISCRIMINATOR, MAGIC_CONTEXT_ID, MAGIC_PROGRAM_ID},
    instruction::{delegate_account, undelegate},
    intent_bundle::MagicIntentBundleBuilder,
    types::DelegateConfig,
    vrf::{
        program_identity_pda, scoped_vrf_identity, RequestRandomness, RequestRandomnessCpi,
        DEFAULT_EPHEMERAL_QUEUE, IDENTITY_SEED, VRF_PROGRAM_ID,
    },
};
use pinocchio::{
    cpi::{Seed, Signer},
    error::ProgramError,
    instruction::InstructionAccount,
    AccountView, Address, ProgramResult,
};
use solana_sha256_hasher::hashv;

use crate::{
    error::LabyrinthError,
    events::{event_chunk_pda, initialize_chunk, validate_event_chunks, write_batch},
    instruction::{
        match_end_winner_at, match_end_winner_count, parse_authority_event, parse_initialize_match,
        parse_kill, parse_match_end, parse_player_event, parse_portal_callback, parse_pubkey,
        parse_room_callback, parse_room_id, IX_COMMIT_AND_UNDELEGATE_MATCH, IX_COMMIT_MATCH,
        IX_DELEGATE_MATCH, IX_ENTER_OR_REVEAL_ROOM, IX_INITIALIZE_CONFIG, IX_INITIALIZE_MATCH,
        IX_RECORD_AUTHORITY_EVENT, IX_RECORD_DEATH, IX_RECORD_KILL, IX_RECORD_MATCH_END,
        IX_RECORD_RESPAWN, IX_REQUEST_MATCH_VRF, IX_RESOLVE_MATCH_VRF, IX_RESOLVE_PORTAL_VRF,
        IX_RESOLVE_ROOM_VRF, IX_SET_BACKEND_AUTHORITY,
    },
    pda::{CONFIG_SEED, EVENT_CHUNK_SEED, MATCH_SEED},
    session::{validate_ranked_match_record, validate_ranked_sessions},
    state::{
        ConfigState, InitializeMatch, MatchState, CONFIG_LEN, MATCH_LEN, MAX_EVENT_CHUNKS,
        MAX_PLAYERS, PHASE_TERMINAL,
    },
    system::create_pda_account,
};

const MATCH_VRF_DOMAIN: &[u8] = b"sol-zero:labyrinth:match-vrf:v1";
const ROOM_VRF_DOMAIN: &[u8] = b"sol-zero:labyrinth:room-vrf:v1";
const PORTAL_VRF_DOMAIN: &[u8] = b"sol-zero:labyrinth:portal-vrf:v1";
const VRF_DATA_BUFFER_LEN: usize = 768;
const MAGIC_DATA_BUFFER_LEN: usize = 1_280;

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
        IX_DELEGATE_MATCH => process_delegate_match(program_id, accounts),
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
        IX_COMMIT_MATCH => process_commit(program_id, accounts, false),
        IX_COMMIT_AND_UNDELEGATE_MATCH => process_commit(program_id, accounts, true),
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
    let args = parse_initialize_match(data)?;
    let player_count = usize::from(args.player_count);
    let event_chunk_count = usize::from(args.event_chunk_count);
    let required_accounts = 6usize
        .checked_add(player_count)
        .and_then(|value| value.checked_add(event_chunk_count))
        .ok_or(ProgramError::InvalidInstructionData)?;
    if accounts.len() != required_accounts {
        return Err(ProgramError::NotEnoughAccountKeys);
    }
    let (fixed, rest) = accounts.split_at(6);
    let payer = &fixed[0];
    let backend = &fixed[1];
    let config_account = &fixed[2];
    let ranked_match_record = &fixed[3];
    let match_account = &fixed[4];
    let system_program = &fixed[5];
    let (session_accounts, event_chunks) = rest.split_at(player_count);
    if !payer.is_signer() || !backend.is_signer() {
        return Err(ProgramError::MissingRequiredSignature);
    }
    let config = load_config(program_id, config_account)?;
    if backend.address().as_array() != &config.backend_authority {
        return Err(ProgramError::IncorrectAuthority);
    }
    let ranked_program = Address::new_from_array(config.ranked_program);
    validate_ranked_match_record(
        &ranked_program,
        ranked_match_record,
        &args.ranked_match_record,
        &args.match_id,
        args.roster,
        player_count,
    )?;
    validate_ranked_sessions(&ranked_program, session_accounts, args.roster)?;
    let (expected_match, bump) =
        Address::derive_program_address(&[MATCH_SEED, &args.match_id], program_id)
            .ok_or(ProgramError::InvalidSeeds)?;
    if match_account.address() != &expected_match || match_account.data_len() != 0 {
        return Err(ProgramError::InvalidSeeds);
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
        let mut match_data = match_account.try_borrow_mut()?;
        let mut state = MatchState::initialize(
            &mut match_data,
            InitializeMatch {
                bump,
                backend_authority: config.backend_authority,
                ranked_program: config.ranked_program,
                args,
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
fn process_delegate_match(program_id: &Address, accounts: &[AccountView]) -> ProgramResult {
    if accounts.len() < 8 {
        return Err(ProgramError::NotEnoughAccountKeys);
    }
    let backend = &accounts[0];
    let owner_program = &accounts[1];
    let system_program = &accounts[2];
    let validator = &accounts[3];
    let match_account = &accounts[4];
    if !backend.is_signer() || owner_program.address() != program_id {
        return Err(ProgramError::MissingRequiredSignature);
    }
    let (match_id, match_bump, chunk_count) = {
        let mut data = match_account.try_borrow_mut()?;
        let mut state = MatchState::load(&mut data)?;
        assert_backend_and_match(program_id, backend, match_account, &state)?;
        state.set_er_validator(*validator.address().as_array());
        (state.match_id(), state.bump(), state.event_chunk_count())
    };
    let expected_len = 4 + (chunk_count + 1) * 4;
    if accounts.len() != expected_len {
        return Err(ProgramError::NotEnoughAccountKeys);
    }
    for group_index in 0..=chunk_count {
        let offset = 4 + group_index * 4;
        let pda = &accounts[offset];
        let buffer = &accounts[offset + 1];
        let delegation_record = &accounts[offset + 2];
        let delegation_metadata = &accounts[offset + 3];
        if group_index == 0 {
            if pda.address() != match_account.address() {
                return Err(ProgramError::InvalidSeeds);
            }
            let seed_parts: [&[u8]; 2] = [MATCH_SEED, &match_id];
            delegate_account(
                &[
                    backend,
                    pda,
                    owner_program,
                    buffer,
                    delegation_record,
                    delegation_metadata,
                    system_program,
                ],
                &seed_parts,
                match_bump,
                DelegateConfig {
                    commit_frequency_ms: u32::MAX,
                    validator: Some(validator.address().clone()),
                },
            )?;
        } else {
            let index = (group_index - 1) as u16;
            let (expected, bump) = event_chunk_pda(program_id, &match_id, index)?;
            if pda.address() != &expected {
                return Err(ProgramError::InvalidSeeds);
            }
            let index_bytes = index.to_le_bytes();
            let seed_parts: [&[u8]; 3] = [EVENT_CHUNK_SEED, &match_id, &index_bytes];
            delegate_account(
                &[
                    backend,
                    pda,
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
            )?;
        }
    }
    Ok(())
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
    undelegate_after: bool,
) -> ProgramResult {
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
    if accounts.len() < 3 {
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

fn expected_match_id(
    match_account: &AccountView,
    program_id: &Address,
) -> Result<[u8; 16], ProgramError> {
    let mut data = match_account.try_borrow_mut()?;
    let state = MatchState::load(&mut data)?;
    assert_match_pda(program_id, match_account, &state)?;
    Ok(state.match_id())
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
