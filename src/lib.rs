#![no_std]

use pinocchio::{AccountView, Address, ProgramResult};

pub mod error;
pub mod events;
pub mod instruction;
pub mod pda;
pub mod processor;
pub mod session;
pub mod signature;
pub mod state;
pub mod system;

pub const ID: Address = Address::new_from_array([
    216, 205, 130, 190, 4, 182, 201, 233, 105, 75, 79, 134, 68, 135, 18, 154, 31, 84, 27, 101, 177,
    215, 29, 158, 175, 109, 246, 142, 189, 233, 249, 170,
]);

#[cfg(feature = "bpf-entrypoint")]
pinocchio::program_entrypoint!(entrypoint_process, 64);

#[cfg(feature = "bpf-entrypoint")]
pinocchio::default_allocator!();

#[cfg(feature = "bpf-entrypoint")]
pinocchio::nostd_panic_handler!();

#[inline(never)]
pub fn entrypoint_process(
    program_id: &Address,
    accounts: &[AccountView],
    instruction_data: &[u8],
) -> ProgramResult {
    processor::process_instruction(program_id, accounts, instruction_data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn program_id_matches_generated_deployment_keypair() {
        assert_eq!(
            ID.to_bytes(),
            [
                216, 205, 130, 190, 4, 182, 201, 233, 105, 75, 79, 134, 68, 135, 18, 154, 31, 84,
                27, 101, 177, 215, 29, 158, 175, 109, 246, 142, 189, 233, 249, 170,
            ]
        );
    }
}
