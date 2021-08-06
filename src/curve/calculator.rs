use crate::error::SwapError;
use std::fmt::Debug;

const INITIAL_SWAP_POOL_AMOUNT: u128 = 1_000_000_000;

pub const TOKENS_IN_POOL: u128 = 2;

pub trait DynPack {
    /// Only required function is to pack given a trait object
    fn pack_into_slice(&self, dst: &mut [u8]);
}

pub fn map_zero_to_none(x: u128) -> Option<u128> {
    if x == 0 {
        None
    } else {
        Some(x)
    }
}

// this will be implemented by each curve slightly differently
// by using a trait we can sub any curve that we like
pub trait CurveCalculator: Debug + DynPack {
    fn validate_supply(&self, token_a_amount: u64, token_b_amount: u64) -> Result<(), SwapError> {
        if token_a_amount == 0 {
            return Err(SwapError::EmptySupply);
        }
        if token_b_amount == 0 {
            return Err(SwapError::EmptySupply);
        }
        Ok(())
    }
    fn new_pool_supply(&self) -> u128 {
        INITIAL_SWAP_POOL_AMOUNT
    }
    fn swap_without_fees(
        &self,
        source_amount: u128,
        swap_source_amount: u128,
        swap_destination_amount: u128,
        trade_direction: TradeDirection,
    ) -> Option<SwapWithoutFeesResult>;

    //essentially performs a withdrawal followed by a swap in order to balance the pool back
    fn withdraw_single_token_type_exact_out(
        &self,
        source_amount: u128,
        swap_token_a_amount: u128,
        swap_token_b_amount: u128,
        pool_supply: u128,
        trade_direction: TradeDirection,
    ) -> Option<u128>;

    /// Some curves function best and prevent attacks if we prevent deposits
    /// after initialization.  For example, the offset curve in `offset.rs`,
    /// which fakes supply on one side of the swap, allows the swap creator
    /// to steal value from all other depositors.
    fn allows_deposits(&self) -> bool {
        true
    }

    /// Get the amount of trading tokens for the given amount of pool tokens,
    /// provided the total trading tokens and supply of pool tokens.
    fn pool_tokens_to_trading_tokens(
        &self,
        pool_tokens: u128,
        pool_token_supply: u128,
        swap_token_a_amount: u128,
        swap_token_b_amount: u128,
        round_direction: RoundDirection,
    ) -> Option<TradingTokenResult>;

    // essentially performs a swap followed by a deposit
    fn deposit_single_token_type(
        &self,
        source_amount: u128,
        swap_token_a_amount: u128,
        swap_token_b_amount: u128,
        pool_supply: u128,
        trade_direction: TradeDirection,
    ) -> Option<u128>;
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TradeDirection {
    AtoB,
    BtoA,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RoundDirection {
    Floor,
    Ceiling,
}

#[derive(Debug, PartialEq)]
pub struct SwapWithoutFeesResult {
    pub source_amount_swapped: u128,
    pub destination_amount_swapped: u128,
}

//used when depositing both tokens
#[derive(Debug, PartialEq)]
pub struct TradingTokenResult {
    pub token_a_amount: u128,
    pub token_b_amount: u128,
}
