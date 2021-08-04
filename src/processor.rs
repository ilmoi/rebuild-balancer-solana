use crate::constraints::{SwapConstraints, SWAP_CONSTRAINTS};
use crate::curve::base::SwapCurve;
use crate::curve::calculator::{RoundDirection, TradeDirection};
use crate::curve::fees::Fees;
use crate::error::SwapError;
use crate::instruction::{DepositAllTokenTypes, Initialize, Swap, SwapInstruction};
use crate::state::{SwapV1, SwapVersion};
use solana_program::account_info::{next_account_info, AccountInfo};
use solana_program::entrypoint::ProgramResult;
use solana_program::msg;
use solana_program::program::invoke_signed;
use solana_program::program_error::ProgramError;
use solana_program::program_pack::Pack;
use solana_program::pubkey::Pubkey;
use std::convert::TryInto;

pub struct Processor {}

impl Processor {
    pub fn unpack_token_account(
        account_info: &AccountInfo,
        token_program_id: &Pubkey,
    ) -> Result<spl_token::state::Account, SwapError> {
        if account_info.owner != token_program_id {
            Err(SwapError::IncorrectTokenProgramId)
        } else {
            spl_token::state::Account::unpack(&account_info.data.borrow())
                .map_err(|_| SwapError::ExpectedAccount)
        }
    }

    pub fn unpack_mint(
        account_info: &AccountInfo,
        token_program_id: &Pubkey,
    ) -> Result<spl_token::state::Mint, SwapError> {
        if account_info.owner != token_program_id {
            Err(SwapError::IncorrectTokenProgramId)
        } else {
            spl_token::state::Mint::unpack(&account_info.data.borrow())
                .map_err(|_| SwapError::ExpectedAccount)
        }
    }

    pub fn token_mint_to<'a>(
        swap: &Pubkey,
        token_program: AccountInfo<'a>,
        mint: AccountInfo<'a>,
        destination: AccountInfo<'a>,
        authority: AccountInfo<'a>,
        nonce: u8,
        amount: u64,
    ) -> Result<(), ProgramError> {
        let swap_bytes = swap.to_bytes();
        let authority_signature_seeds = [&swap_bytes[..32], &[nonce]];
        let signers = &[&authority_signature_seeds[..]];
        let ix = spl_token::instruction::mint_to(
            token_program.key,
            mint.key,
            destination.key,
            authority.key,
            &[],
            amount,
        )?;

        invoke_signed(&ix, &[mint, destination, authority, token_program], signers)
    }

    pub fn token_transfer<'a>(
        swap: &Pubkey,
        token_program: AccountInfo<'a>,
        source: AccountInfo<'a>,
        destination: AccountInfo<'a>,
        authority: AccountInfo<'a>,
        nonce: u8,
        amount: u64,
    ) -> Result<(), ProgramError> {
        let swap_bytes = swap.to_bytes();
        let authority_signature_seeds = [&swap_bytes[..32], &[nonce]];
        let signers = &[&authority_signature_seeds[..]];
        let ix = spl_token::instruction::transfer(
            token_program.key,
            source.key,
            destination.key,
            authority.key,
            &[],
            amount,
        )?;
        invoke_signed(
            &ix,
            &[source, destination, authority, token_program],
            signers,
        )
    }

    // ----------------------------------------------------------------------------- processors

    // 1)checks a bunch, 2)mints tokens into dest acc, 3)saves state into swap_info acc
    pub fn process_initialize(
        program_id: &Pubkey,
        nonce: u8,
        fees: Fees,
        swap_curve: SwapCurve,
        accounts: &[AccountInfo],
        swap_constraints: &Option<SwapConstraints>,
    ) -> ProgramResult {
        let account_info_iter = &mut accounts.iter();
        let swap_info = next_account_info(account_info_iter)?; //this will hold the state for a given pool, eg RAY-SOL
        let authority_info = next_account_info(account_info_iter)?; //authority over pool tokens, todo derived from the token swap program?
        let token_a_info = next_account_info(account_info_iter)?; //account that will hold token A for the pool
        let token_b_info = next_account_info(account_info_iter)?; //account that will hold token B for the pool
        let pool_mint_info = next_account_info(account_info_iter)?; //this is the mint address for the pool tokens - eg lpRAYSOL whatever
        let fee_account_info = next_account_info(account_info_iter)?; //this is where the fees will accrue
        let destination_info = next_account_info(account_info_iter)?; //this is where the pool tokens will be initially minted into
        let token_program_info = next_account_info(account_info_iter)?;

        let token_program_id = *token_program_info.key;

        let token_a = Self::unpack_token_account(token_a_info, &token_program_id)?;
        let token_b = Self::unpack_token_account(token_b_info, &token_program_id)?;
        let fee_account = Self::unpack_token_account(fee_account_info, &token_program_id)?;
        let destination = Self::unpack_token_account(destination_info, &token_program_id)?;
        let pool_mint = Self::unpack_mint(pool_mint_info, &token_program_id)?;

        // check that both accounts A and B have some initial tokens in them
        // todo why is this necessary? I suspect this is in place to ensure they exist and don't have to be created
        swap_curve
            .calculator
            .validate_supply(token_a.amount, token_b.amount)?;

        // check that
        // 1)curve is one of allowed types and
        // 2)fees are reasonable (numerator has to be higher or above)
        if let Some(swap_constraints) = swap_constraints {
            let owner_key = swap_constraints
                .owner_key
                .parse::<Pubkey>()
                .map_err(|_| SwapError::InvalidOwner)?;
            if fee_account.owner != owner_key {
                return Err(SwapError::InvalidOwner.into());
            }
            swap_constraints.validate_curve(&swap_curve)?;
            swap_constraints.validate_fees(&fees)?;
        }

        //checks fee denominators aren't 0 and that numerator < denominator
        fees.validate()?;

        //initial amount of tokens in pool is a constant of 1_000_000_000
        let initial_amount = swap_curve.calculator.new_pool_supply();

        //invokes the spl program to mint tokens
        Self::token_mint_to(
            swap_info.key,
            token_program_info.clone(),
            pool_mint_info.clone(),
            destination_info.clone(), //mints to destination addr
            authority_info.clone(),
            nonce,
            to_u64(initial_amount)?,
        )?;

        // create the state for the given pool
        let obj = SwapVersion::SwapV1(SwapV1 {
            is_initialized: true,
            nonce,
            token_program_id,
            token_a: *token_a_info.key,
            token_b: *token_b_info.key,
            pool_mint: *pool_mint_info.key,
            token_a_mint: token_a.mint,
            token_b_mint: token_b.mint,
            pool_fee_account: *fee_account_info.key,
            fees,
            swap_curve,
        });
        // packs that state into the data of the swap_info account
        SwapVersion::pack(obj, &mut swap_info.data.borrow_mut())?;
        Ok(())
    }

    pub fn process_swap(
        program_id: &Pubkey,
        amount_in: u64,
        minimum_amount_out: u64,
        accounts: &[AccountInfo],
    ) -> ProgramResult {
        let account_info_iter = &mut accounts.iter();

        let swap_info = next_account_info(account_info_iter)?; //state of the pool
        let authority_info = next_account_info(account_info_iter)?; //authority over pool tokens that can mint and move them around
        let user_transfer_authority_info = next_account_info(account_info_iter)?; //temp pubkey created by the user that has the right to move a precise amount of tokens from their acc

        let source_info = next_account_info(account_info_iter)?; //A token account, belongs to USER (will lose balance)
        let swap_source_info = next_account_info(account_info_iter)?; //A Token account, belongs to exchange (will gain balance)
        let swap_destination_info = next_account_info(account_info_iter)?; //B Token account, belongs to exchange (will lose balance)
        let destination_info = next_account_info(account_info_iter)?; //B Token account, belongs to USER (will gain balance)

        let pool_mint_info = next_account_info(account_info_iter)?; //mint addr of the pool token
        let pool_fee_account_info = next_account_info(account_info_iter)?; //where fees accrue
        let token_program_info = next_account_info(account_info_iter)?;

        //unpack the state of the pool
        let token_swap = SwapVersion::unpack(&swap_info.data.borrow())?;

        //unpack exchange's accounts
        let source_account =
            Self::unpack_token_account(swap_source_info, token_swap.token_program_id())?;
        let dest_account =
            Self::unpack_token_account(swap_destination_info, token_swap.token_program_id())?;

        // unpack the mint token account for the pool token
        let pool_mint = Self::unpack_mint(pool_mint_info, token_swap.token_program_id())?;

        //if the exchange's A account = the A account stored in the state, then A->B, else B->A
        let trade_direction = if *swap_source_info.key == *token_swap.token_a_account() {
            TradeDirection::AtoB
        } else {
            TradeDirection::BtoA
        };

        // ----------------------------------------------------------------------------- calculation

        //do the actual swap
        let result = token_swap
            .swap_curve()
            .swap(
                to_u128(amount_in)?,
                to_u128(source_account.amount)?,
                to_u128(dest_account.amount)?,
                trade_direction,
                token_swap.fees(),
            )
            .ok_or(SwapError::ZeroTradingTokens)?;

        // check for slippage
        if result.destination_amount_swapped < to_u128(minimum_amount_out)? {
            return Err(SwapError::ExceededSlippage.into());
        }

        // depending on trade direction, these are the new balance of X and Y tokens in the pool
        let (swap_token_a_amount, swap_token_b_amount) = match trade_direction {
            TradeDirection::AtoB => (
                result.new_swap_source_amount,
                result.new_swap_destination_amount,
            ),
            TradeDirection::BtoA => (
                result.new_swap_destination_amount,
                result.new_swap_source_amount,
            ),
        };

        // ----------------------------------------------------------------------------- execution

        // move X token from USER -> exchange
        Self::token_transfer(
            swap_info.key,
            token_program_info.clone(),
            source_info.clone(),
            swap_source_info.clone(),
            user_transfer_authority_info.clone(),
            token_swap.nonce(),
            to_u64(result.source_amount_swapped)?,
        )?;

        // we earned a fee as an exchange for performing the swap
        // however the fee is denominated in X tokens
        // we don't want to withdraw X tokens, we want to withdraw POOL tokens
        // so we convert X tokens to pool tokens using a special ratio from the balancer paper
        // now this pool token amount can be split between all the parties that deserve it
        // todo be sure to visualize this in the FE
        let mut pool_token_amount = token_swap
            .swap_curve()
            .withdraw_single_token_type_exact_out(
                result.owner_fee,
                swap_token_a_amount,
                swap_token_b_amount,
                to_u128(pool_mint.supply)?,
                trade_direction,
                token_swap.fees(),
            )
            .ok_or(SwapError::FeeCalculationFailure)?;

        if pool_token_amount > 0 {
            // if host is present
            if let Ok(host_fee_account_info) = next_account_info(account_info_iter) {
                let host_fee_account = Self::unpack_token_account(
                    host_fee_account_info,
                    token_swap.token_program_id(),
                )?;
                if *pool_mint_info.key != host_fee_account.mint {
                    return Err(SwapError::IncorrectPoolMint.into());
                }
                let host_fee = token_swap
                    .fees()
                    .host_fee(pool_token_amount)
                    .ok_or(SwapError::FeeCalculationFailure)?;
                if host_fee > 0 {
                    //the first fee we subtract and send to the pool host (the UI)
                    pool_token_amount = pool_token_amount
                        .checked_sub(host_fee)
                        .ok_or(SwapError::FeeCalculationFailure)?;
                    //mint tokens to host
                    Self::token_mint_to(
                        swap_info.key,
                        token_program_info.clone(),
                        pool_mint_info.clone(),
                        host_fee_account_info.clone(),
                        authority_info.clone(),
                        token_swap.nonce(),
                        to_u64(host_fee)?,
                    )?;
                }
            }
            //mint tokens to exchange - todo which I guess are gonna be further divided between exchange and the LPs
            Self::token_mint_to(
                swap_info.key,
                token_program_info.clone(),
                pool_mint_info.clone(),
                pool_fee_account_info.clone(),
                authority_info.clone(),
                token_swap.nonce(),
                to_u64(pool_token_amount)?, //this is original pool_token_amont LESS host fees
            )?;
        }

        //finally in the end send the user their Y tokens
        Self::token_transfer(
            swap_info.key,
            token_program_info.clone(),
            swap_destination_info.clone(),
            destination_info.clone(),
            authority_info.clone(),
            token_swap.nonce(),
            to_u64(result.destination_amount_swapped)?,
        )?;

        Ok(())
    }

    pub fn process_deposit_all_token_types(
        program_id: &Pubkey,
        pool_token_amount: u64, //todo weird, so we're specifying pool tokens... why? Maybe this is our way of signaling what % of the pool the user should own
        maximum_token_a_amount: u64, //for the purposes of slippage
        maximum_token_b_amount: u64, //for the purposes of slippage
        accounts: &[AccountInfo],
    ) -> ProgramResult {
        let account_info_iter = &mut accounts.iter();
        let swap_info = next_account_info(account_info_iter)?;
        let authority_info = next_account_info(account_info_iter)?;

        let user_transfer_authority_info = next_account_info(account_info_iter)?; //authority over user's A and B accounts
        let source_a_info = next_account_info(account_info_iter)?; //user's A account
        let source_b_info = next_account_info(account_info_iter)?; //user's B account
        let token_a_info = next_account_info(account_info_iter)?; //exchange A account
        let token_b_info = next_account_info(account_info_iter)?; //exchange B account

        let pool_mint_info = next_account_info(account_info_iter)?;
        let dest_info = next_account_info(account_info_iter)?;
        let token_program_info = next_account_info(account_info_iter)?;

        let token_swap = SwapVersion::unpack(&swap_info.data.borrow())?;
        let calculator = &token_swap.swap_curve().calculator;

        if !calculator.allows_deposits() {
            return Err(SwapError::UnsupportedCurveOperation.into());
        }

        let token_a = Self::unpack_token_account(token_a_info, token_swap.token_program_id())?;
        let token_b = Self::unpack_token_account(token_b_info, token_swap.token_program_id())?;
        let pool_mint = Self::unpack_mint(pool_mint_info, token_swap.token_program_id())?;
        let current_pool_mint_supply = to_u128(pool_mint.supply)?;

        //get the outstanding + max pool token supply
        let (pool_token_amount, pool_mint_supply) = if current_pool_mint_supply > 0 {
            (to_u128(pool_token_amount)?, current_pool_mint_supply)
        } else {
            //if the current supply is 0, means we're funding a new pool, then by definition we're going to have 100% of it, so the two values are the same
            (calculator.new_pool_supply(), calculator.new_pool_supply())
        };

        // ----------------------------------------------------------------------------- calc

        // token X amount to deposit, token Y amount to deposit
        let results = calculator
            .pool_tokens_to_trading_tokens(
                pool_token_amount,        //outstanding pool token amount
                pool_mint_supply,         //total pool token amount
                to_u128(token_a.amount)?, //exchange A tokens
                to_u128(token_b.amount)?, //exchange B tokens
                RoundDirection::Ceiling,
            )
            .ok_or(SwapError::ZeroTradingTokens)?;

        let token_a_amount = to_u64(results.token_a_amount)?;
        if token_a_amount > maximum_token_a_amount {
            return Err(SwapError::ExceededSlippage.into());
        }
        if token_a_amount == 0 {
            return Err(SwapError::ZeroTradingTokens.into());
        }

        let token_b_amount = to_u64(results.token_b_amount)?;
        if token_b_amount > maximum_token_b_amount {
            return Err(SwapError::ExceededSlippage.into());
        }
        if token_b_amount == 0 {
            return Err(SwapError::ZeroTradingTokens.into());
        }

        // ----------------------------------------------------------------------------- execute

        let pool_token_amount = to_u64(pool_token_amount)?;

        // transfer token X into the exchange
        Self::token_transfer(
            swap_info.key,
            token_program_info.clone(),
            source_a_info.clone(),
            token_a_info.clone(),
            user_transfer_authority_info.clone(),
            token_swap.nonce(),
            token_a_amount,
        )?;
        // transfer token Y into the exchange
        Self::token_transfer(
            swap_info.key,
            token_program_info.clone(),
            source_b_info.clone(),
            token_b_info.clone(),
            user_transfer_authority_info.clone(),
            token_swap.nonce(),
            token_b_amount,
        )?;

        // mint POOL tokens back to the user, that he'll be able to stake in the LP farm
        Self::token_mint_to(
            swap_info.key,
            token_program_info.clone(),
            pool_mint_info.clone(),
            dest_info.clone(), //user's destination addr
            authority_info.clone(),
            token_swap.nonce(),
            pool_token_amount, //we started this function call by specifying how many we'd like to get back
        )?;

        Ok(())
    }

    // ----------------------------------------------------------------------------- triage

    pub fn process(
        program_id: &Pubkey,
        accounts: &[AccountInfo],
        instruction_data: &[u8],
    ) -> ProgramResult {
        Self::process_with_constraints(program_id, accounts, instruction_data, &SWAP_CONSTRAINTS)
    }

    pub fn process_with_constraints(
        program_id: &Pubkey,
        accounts: &[AccountInfo],
        instruction_data: &[u8],
        swap_constraints: &Option<SwapConstraints>,
    ) -> ProgramResult {
        let ix = SwapInstruction::unpack(instruction_data)?;

        match ix {
            SwapInstruction::Initialize(Initialize {
                nonce,
                fees,
                swap_curve,
            }) => {
                msg!("Instruction: Init");
                Self::process_initialize(
                    program_id,
                    nonce,
                    fees,
                    swap_curve,
                    accounts,
                    swap_constraints,
                )
            }
            SwapInstruction::Swap(Swap {
                amount_in,
                minimum_amount_out,
            }) => {
                msg!("Instruction: Swap");
                Self::process_swap(program_id, amount_in, minimum_amount_out, accounts)
            }
            SwapInstruction::DepositAllTokenTypes(DepositAllTokenTypes {
                pool_token_amount,
                maximum_token_a_amount,
                maximum_token_b_amount,
            }) => {
                msg!("Instruction: DepositAllTokenTypes");
                Self::process_deposit_all_token_types(
                    program_id,
                    pool_token_amount,
                    maximum_token_a_amount,
                    maximum_token_b_amount,
                    accounts,
                )
            }
            _ => Ok(()),
        }
    }
}

fn to_u128(val: u64) -> Result<u128, SwapError> {
    val.try_into().map_err(|_| SwapError::ConversionFailure)
}

fn to_u64(val: u128) -> Result<u64, SwapError> {
    val.try_into().map_err(|_| SwapError::ConversionFailure)
}