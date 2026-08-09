//! Minimal verifier for a backend launch permit.  The backend signs, but it
//! never signs the transaction or pays account rent.

use pinocchio::{
    error::ProgramError, sysvars::instructions::Instructions, AccountView, Address, ProgramResult,
};

const ED25519_PROGRAM_ID: Address = Address::new_from_array([
    0x03, 0x7d, 0x46, 0xd6, 0x7c, 0x93, 0xfb, 0xbe, 0x12, 0xf9, 0x42, 0x8f, 0x83, 0x8d, 0x40, 0xff,
    0x05, 0x70, 0x74, 0x49, 0x27, 0xf4, 0x8a, 0x64, 0xfc, 0xca, 0x70, 0x44, 0x80, 0x00, 0x00, 0x00,
]);
const CURRENT_IX: u16 = u16::MAX;
const HEADER_LEN: usize = 16;

/// Requires an Ed25519 precompile instruction before the Labyrinth
/// instruction in the same transaction.
pub fn verify_prior_ed25519_instruction(
    instructions_sysvar: &AccountView,
    signer: &[u8; 32],
    signature: &[u8; 64],
    message: &[u8],
) -> ProgramResult {
    verify_prior_ed25519(instructions_sysvar, signer, Some(signature), message)
}

/// Verifies the public key and message of a prior Ed25519 precompile. The
/// signature exists only in that precompile instruction and is not duplicated
/// in Labyrinth instruction data.
pub fn verify_prior_ed25519_message(
    instructions_sysvar: &AccountView,
    signer: &[u8; 32],
    message: &[u8],
) -> ProgramResult {
    verify_prior_ed25519(instructions_sysvar, signer, None, message)
}

fn verify_prior_ed25519(
    instructions_sysvar: &AccountView,
    signer: &[u8; 32],
    signature: Option<&[u8; 64]>,
    message: &[u8],
) -> ProgramResult {
    let instructions = Instructions::try_from(instructions_sysvar)?;
    for index in 0..instructions.load_current_index() as usize {
        let instruction = instructions.load_instruction_at(index)?;
        if instruction.get_program_id() != &ED25519_PROGRAM_ID {
            continue;
        }
        let data = instruction.get_instruction_data();
        if data.len() < HEADER_LEN || data[0] != 1 {
            continue;
        }
        let read = |offset: usize| -> Result<u16, ProgramError> {
            data.get(offset..offset + 2)
                .ok_or(ProgramError::InvalidInstructionData)?
                .try_into()
                .map(u16::from_le_bytes)
                .map_err(|_| ProgramError::InvalidInstructionData)
        };
        let signature_offset = read(2)? as usize;
        let signature_ix = read(4)?;
        let signer_offset = read(6)? as usize;
        let signer_ix = read(8)?;
        let message_offset = read(10)? as usize;
        let message_len = read(12)? as usize;
        let message_ix = read(14)?;
        if signature_ix != CURRENT_IX || signer_ix != CURRENT_IX || message_ix != CURRENT_IX {
            continue;
        }
        let matches = data
            .get(signature_offset..signature_offset + 64)
            .zip(data.get(signer_offset..signer_offset + 32))
            .zip(data.get(message_offset..message_offset + message_len))
            .map(|((actual_signature, actual_signer), actual_message)| {
                signature
                    .map(|expected| actual_signature == expected)
                    .unwrap_or(true)
                    && actual_signer == signer
                    && actual_message == message
            })
            .unwrap_or(false);
        if matches {
            return Ok(());
        }
    }
    Err(ProgramError::InvalidInstructionData)
}
