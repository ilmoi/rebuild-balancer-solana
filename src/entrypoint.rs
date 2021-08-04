use crate::error::SwapError;
use crate::processor::Processor;
use solana_program::account_info::AccountInfo;
use solana_program::entrypoint;
use solana_program::entrypoint::ProgramResult;
use solana_program::program_error::PrintProgramError;
use solana_program::pubkey::Pubkey;

entrypoint!(process_instruction);
fn process_instruction(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    instruction_data: &[u8],
) -> ProgramResult {
    if let Err(e) = Processor::process(program_id, accounts, instruction_data) {
        e.print::<SwapError>();
        return Err(e);
    }
    Ok(())
}
