use crate::error::SwapError;
use arrayref::{array_mut_ref, array_ref, array_refs, mut_array_refs};
use solana_program::program_error::ProgramError;
use solana_program::program_pack::{IsInitialized, Pack, Sealed};
use std::convert::TryFrom;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Fees {
    //to LPs - EXCLUDING THE OWNER FEES, the two are calced and handled separately
    pub trade_fee_numerator: u64,
    pub trade_fee_denominator: u64,
    //to swap ctr owner
    pub owner_trade_fee_numerator: u64,
    pub owner_trade_fee_denominator: u64,
    /// Owner withdraw fees are extra liquidity pool token amounts that are sent to the owner on every withdrawal.
    /// these are modelled based on balancer - https://balancer.fi/whitepaper.pdf (see swap and exit fees)
    pub owner_withdraw_fee_numerator: u64,
    pub owner_withdraw_fee_denominator: u64,
    //to UI host - taken as proportion of owner trading fees
    pub host_fee_numerator: u64,
    pub host_fee_denominator: u64,
}

impl Fees {
    pub fn validate(&self) -> Result<(), SwapError> {
        validate_fraction(self.trade_fee_numerator, self.trade_fee_denominator)?;
        validate_fraction(
            self.owner_trade_fee_numerator,
            self.owner_trade_fee_denominator,
        )?;
        validate_fraction(
            self.owner_withdraw_fee_numerator,
            self.owner_withdraw_fee_denominator,
        )?;
        validate_fraction(self.host_fee_numerator, self.host_fee_denominator)?;
        Ok(())
    }

    /// Calculate the withdraw fee in pool tokens - used when double sided withdrawals are called
    pub fn owner_withdraw_fee(&self, pool_tokens: u128) -> Option<u128> {
        calculate_fee(
            pool_tokens,
            u128::try_from(self.owner_withdraw_fee_numerator).ok()?,
            u128::try_from(self.owner_withdraw_fee_denominator).ok()?,
        )
    }

    /// Calculate the trading fee in trading tokens
    pub fn trading_fee(&self, trading_tokens: u128) -> Option<u128> {
        calculate_fee(
            trading_tokens,
            u128::try_from(self.trade_fee_numerator).ok()?,
            u128::try_from(self.trade_fee_denominator).ok()?,
        )
    }

    /// Calculate the owner trading fee in trading tokens
    pub fn owner_trading_fee(&self, trading_tokens: u128) -> Option<u128> {
        calculate_fee(
            trading_tokens,
            u128::try_from(self.owner_trade_fee_numerator).ok()?,
            u128::try_from(self.owner_trade_fee_denominator).ok()?,
        )
    }

    /// Calculate the host fee based on the owner fee, only used in production
    /// situations where a program is hosted by multiple frontends
    pub fn host_fee(&self, owner_fee: u128) -> Option<u128> {
        calculate_fee(
            owner_fee,
            u128::try_from(self.host_fee_numerator).ok()?,
            u128::try_from(self.host_fee_denominator).ok()?,
        )
    }
}

// fee = amount * numerator / denominator
pub fn calculate_fee(
    token_amount: u128,
    fee_numerator: u128,
    fee_denominator: u128,
) -> Option<u128> {
    if fee_numerator == 0 || token_amount == 0 {
        Some(0)
    } else {
        let fee = token_amount
            .checked_mul(fee_numerator)?
            .checked_div(fee_denominator)?;
        if fee == 0 {
            Some(1) // minimum fee of one token
        } else {
            Some(fee)
        }
    }
}

fn validate_fraction(numerator: u64, denominator: u64) -> Result<(), SwapError> {
    if denominator == 0 && numerator == 0 {
        Ok(())
    } else if numerator >= denominator {
        Err(SwapError::InvalidFee)
    } else {
        Ok(())
    }
}

// ----------------------------------------------------------------------------- program pack

/// IsInitialized is required to use `Pack::pack` and `Pack::unpack`
impl IsInitialized for Fees {
    fn is_initialized(&self) -> bool {
        true
    }
}

impl Sealed for Fees {}
impl Pack for Fees {
    const LEN: usize = 64;
    fn pack_into_slice(&self, output: &mut [u8]) {
        let output = array_mut_ref![output, 0, 64];
        let (
            trade_fee_numerator,
            trade_fee_denominator,
            owner_trade_fee_numerator,
            owner_trade_fee_denominator,
            owner_withdraw_fee_numerator,
            owner_withdraw_fee_denominator,
            host_fee_numerator,
            host_fee_denominator,
        ) = mut_array_refs![output, 8, 8, 8, 8, 8, 8, 8, 8];
        *trade_fee_numerator = self.trade_fee_numerator.to_le_bytes();
        *trade_fee_denominator = self.trade_fee_denominator.to_le_bytes();
        *owner_trade_fee_numerator = self.owner_trade_fee_numerator.to_le_bytes();
        *owner_trade_fee_denominator = self.owner_trade_fee_denominator.to_le_bytes();
        *owner_withdraw_fee_numerator = self.owner_withdraw_fee_numerator.to_le_bytes();
        *owner_withdraw_fee_denominator = self.owner_withdraw_fee_denominator.to_le_bytes();
        *host_fee_numerator = self.host_fee_numerator.to_le_bytes();
        *host_fee_denominator = self.host_fee_denominator.to_le_bytes();
    }

    fn unpack_from_slice(input: &[u8]) -> Result<Fees, ProgramError> {
        let input = array_ref![input, 0, 64];
        #[allow(clippy::ptr_offset_with_cast)]
        let (
            trade_fee_numerator,
            trade_fee_denominator,
            owner_trade_fee_numerator,
            owner_trade_fee_denominator,
            owner_withdraw_fee_numerator,
            owner_withdraw_fee_denominator,
            host_fee_numerator,
            host_fee_denominator,
        ) = array_refs![input, 8, 8, 8, 8, 8, 8, 8, 8];
        Ok(Self {
            trade_fee_numerator: u64::from_le_bytes(*trade_fee_numerator),
            trade_fee_denominator: u64::from_le_bytes(*trade_fee_denominator),
            owner_trade_fee_numerator: u64::from_le_bytes(*owner_trade_fee_numerator),
            owner_trade_fee_denominator: u64::from_le_bytes(*owner_trade_fee_denominator),
            owner_withdraw_fee_numerator: u64::from_le_bytes(*owner_withdraw_fee_numerator),
            owner_withdraw_fee_denominator: u64::from_le_bytes(*owner_withdraw_fee_denominator),
            host_fee_numerator: u64::from_le_bytes(*host_fee_numerator),
            host_fee_denominator: u64::from_le_bytes(*host_fee_denominator),
        })
    }
}
