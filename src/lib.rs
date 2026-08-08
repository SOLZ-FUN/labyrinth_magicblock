#![no_std]

use pinocchio::{AccountView, Address, ProgramResult};

pub mod error;
pub mod events;
pub mod instruction;
pub mod pda;
pub mod processor;
pub mod session;
pub mod state;
pub mod system;

pub const ID: Address = Address::new_from_array([
    218, 209, 63, 32, 116, 73, 185, 195, 164, 149, 119, 13, 34, 101, 162, 137, 194, 46, 170, 160,
    199, 232, 16, 85, 208, 99, 138, 170, 144, 249, 118, 49,
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
                218, 209, 63, 32, 116, 73, 185, 195, 164, 149, 119, 13, 34, 101, 162, 137, 194, 46,
                170, 160, 199, 232, 16, 85, 208, 99, 138, 170, 144, 249, 118, 49,
            ]
        );
    }
}
