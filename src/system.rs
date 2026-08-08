use pinocchio::{
    cpi::{Seed, Signer},
    error::ProgramError,
    sysvars::{rent::Rent, Sysvar},
    AccountView, Address, ProgramResult,
};
use pinocchio_system::{instructions::CreateAccount, ID as SYSTEM_PROGRAM_ID};

pub fn create_pda_account(
    payer: &AccountView,
    account: &AccountView,
    system_program: &AccountView,
    owner: &Address,
    space: usize,
    signer_seeds: &[Seed],
) -> ProgramResult {
    if !payer.is_signer() {
        return Err(ProgramError::MissingRequiredSignature);
    }
    if !payer.is_writable() || !account.is_writable() {
        return Err(ProgramError::InvalidAccountData);
    }
    if system_program.address() != &SYSTEM_PROGRAM_ID {
        return Err(ProgramError::IncorrectProgramId);
    }
    let lamports = Rent::get()?.try_minimum_balance(space)?;
    CreateAccount {
        from: payer,
        to: account,
        lamports,
        space: space as u64,
        owner,
    }
    .invoke_signed(&[Signer::from(signer_seeds)])
}
