use crate::curve::calculator::{
    map_zero_to_none, CurveCalculator, DynPack, RoundDirection, SwapWithoutFeesResult,
    TradeDirection, TradingTokenResult,
};
use solana_program::program_error::ProgramError;
use solana_program::program_pack::{IsInitialized, Pack, Sealed};
use spl_math::checked_ceil_div::CheckedCeilDiv;
use spl_math::precise_number::PreciseNumber;

// this is the struct that's going to implement the Calculator trait
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ConstantProductCurve;

impl CurveCalculator for ConstantProductCurve {
    // constant product swap, x * y = constant
    fn swap_without_fees(
        &self,
        source_amount: u128,
        swap_source_amount: u128,
        swap_destination_amount: u128,
        trade_direction: TradeDirection,
    ) -> Option<SwapWithoutFeesResult> {
        swap(source_amount, swap_source_amount, swap_destination_amount)
    }

    fn withdraw_single_token_type_exact_out(
        &self,
        source_amount: u128, //source tokens that go to the OWNER as a fee for executing the trade LESS FEE
        swap_token_a_amount: u128,
        swap_token_b_amount: u128,
        pool_supply: u128,
        trade_direction: TradeDirection,
    ) -> Option<u128> {
        withdraw_single_token_type_exact_out(
            source_amount, //source tokens that go to the OWNER as a fee for executing the trade LESS FEE
            swap_token_a_amount,
            swap_token_b_amount,
            pool_supply,
            trade_direction,
            RoundDirection::Ceiling,
        )
    }

    /// The constant product implementation is a simple ratio calculation for how many
    /// trading tokens correspond to a certain number of pool tokens
    fn pool_tokens_to_trading_tokens(
        &self,
        pool_tokens: u128,
        pool_token_supply: u128,
        swap_token_a_amount: u128,
        swap_token_b_amount: u128,
        round_direction: RoundDirection,
    ) -> Option<TradingTokenResult> {
        pool_tokens_to_trading_tokens(
            pool_tokens,
            pool_token_supply,
            swap_token_a_amount,
            swap_token_b_amount,
            round_direction,
        )
    }
}

// ----------------------------------------------------------------------------- helper fns
// the only reason these are separate functions and not methods is so that they can be re-used outside of constant product curve

pub fn swap(
    source_amount: u128,
    swap_source_amount: u128,
    swap_destination_amount: u128,
) -> Option<SwapWithoutFeesResult> {
    // swap_ = EXISTING tokens in the pool
    // product = existing X * existing Y
    let invariant = swap_source_amount.checked_mul(swap_destination_amount)?;

    // new pool tokens X = existing pool tokens X + added tokens X
    let new_swap_source_amount = swap_source_amount.checked_add(source_amount)?;

    // performs weird division s.t. neither of the two values are truncated down, and instead both are CEILed
    // https://docs.rs/spl-math/0.1.0/src/spl_math/checked_ceil_div.rs.html#25
    // new pool tokens Y, new pool tokens X (updated) = product / new pool tokens X
    let (new_swap_destination_amount, new_swap_source_amount) =
        invariant.checked_ceil_div(new_swap_source_amount)?;

    // how many X tokens user loses = new X pool - old X pool
    let source_amount_swapped = new_swap_source_amount.checked_sub(swap_source_amount)?;

    // how many Y tokens user gains = old Y pool - new Y pool
    let destination_amount_swapped =
        map_zero_to_none(swap_destination_amount.checked_sub(new_swap_destination_amount)?)?;

    Some(SwapWithoutFeesResult {
        source_amount_swapped,      //how many X tokens user loses
        destination_amount_swapped, //how many Y tokens user gains
    })
}

/// based on this -> https://balancer.finance/whitepaper/#single-asset-withdrawal
pub fn withdraw_single_token_type_exact_out(
    source_amount: u128, //source tokens that go to the OWNER as a fee for executing the trade LESS FEE. this will be the numerator
    swap_token_a_amount: u128,
    swap_token_b_amount: u128,
    pool_supply: u128,
    trade_direction: TradeDirection,
    round_direction: RoundDirection,
) -> Option<u128> {
    //this will be the denominator
    let swap_source_amount = match trade_direction {
        TradeDirection::AtoB => swap_token_a_amount,
        TradeDirection::BtoA => swap_token_b_amount,
    };
    // Struct encapsulating a fixed-point number that allows for decimal calculations
    // https://docs.rs/spl-math/0.1.0/spl_math/precise_number/struct.PreciseNumber.html
    let swap_source_amount = PreciseNumber::new(swap_source_amount)?;
    let source_amount = PreciseNumber::new(source_amount)?;
    let ratio = source_amount.checked_div(&swap_source_amount)?; //fees in X token / total X tokens in pool
    let one = PreciseNumber::new(1)?;

    //the math from balancer paper - we rebalance the pool after subtracting one token
    let base = one.checked_sub(&ratio)?; //1-r
    let root = one.checked_sub(&base.sqrt()?)?; //1 - √(1-r)
    let pool_supply = PreciseNumber::new(pool_supply)?;

    //we're multiplying the POOL token supply by the ratio, coz we want to withdraw POOL tokens as fee, not actual X tokens
    let pool_tokens = pool_supply.checked_mul(&root)?;

    match round_direction {
        RoundDirection::Floor => pool_tokens.floor()?.to_imprecise(), //back to u128
        RoundDirection::Ceiling => pool_tokens.ceiling()?.to_imprecise(), //back to u128
    }
}

pub fn pool_tokens_to_trading_tokens(
    pool_tokens: u128,         //outstanding pool token amount
    pool_token_supply: u128,   //total pool tokena mount
    swap_token_a_amount: u128, //exchange A tokens
    swap_token_b_amount: u128, //exchange B tokens
    round_direction: RoundDirection,
) -> Option<TradingTokenResult> {
    //outstanding P / total P * exchange A tokens
    let mut token_a_amount = pool_tokens
        .checked_mul(swap_token_a_amount)?
        .checked_div(pool_token_supply)?;
    //outstanding P / total P * exchange B tokens
    let mut token_b_amount = pool_tokens
        .checked_mul(swap_token_b_amount)?
        .checked_div(pool_token_supply)?;

    let (token_a_amount, token_b_amount) = match round_direction {
        RoundDirection::Floor => (token_a_amount, token_b_amount),
        RoundDirection::Ceiling => {
            let token_a_remainder = pool_tokens
                .checked_mul(swap_token_a_amount)?
                .checked_rem(pool_token_supply)?;
            // Also check for 0 token A and B amount to avoid taking too much
            // for tiny amounts of pool tokens.  For example, if someone asks
            // for 1 pool token, which is worth 0.01 token A, we avoid the
            // ceiling of taking 1 token A and instead return 0, for it to be
            // rejected later in processing.
            if token_a_remainder > 0 && token_a_amount > 0 {
                token_a_amount += 1;
            }
            let token_b_remainder = pool_tokens
                .checked_mul(swap_token_b_amount)?
                .checked_rem(pool_token_supply)?;
            if token_b_remainder > 0 && token_b_amount > 0 {
                token_b_amount += 1;
            }
            (token_a_amount, token_b_amount)
        }
    };
    Some(TradingTokenResult {
        token_a_amount,
        token_b_amount,
    })
}

// ----------------------------------------------------------------------------- program pack

/// IsInitialized is required to use `Pack::pack` and `Pack::unpack`
impl IsInitialized for ConstantProductCurve {
    fn is_initialized(&self) -> bool {
        true
    }
}
impl Sealed for ConstantProductCurve {}
impl Pack for ConstantProductCurve {
    const LEN: usize = 0;
    fn pack_into_slice(&self, output: &mut [u8]) {
        (self as &dyn DynPack).pack_into_slice(output);
    }

    fn unpack_from_slice(_input: &[u8]) -> Result<ConstantProductCurve, ProgramError> {
        Ok(Self {})
    }
}

impl DynPack for ConstantProductCurve {
    fn pack_into_slice(&self, _output: &mut [u8]) {}
}
