use crate::curve::calculator::{CurveCalculator, SwapWithoutFeesResult, TradeDirection};
use crate::curve::constant_product::ConstantProductCurve;
use crate::curve::fees::Fees;
use arrayref::{array_mut_ref, array_ref, array_refs, mut_array_refs};
use solana_program::program_error::ProgramError;
use solana_program::program_pack::{Pack, Sealed};
use std::convert::{TryFrom, TryInto};

//list of possible curves
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CurveType {
    ConstantProduct,
}

//chooses one curve and links the relevant Calculator trait implementation
#[derive(Debug)]
pub struct SwapCurve {
    pub curve_type: CurveType,
    pub calculator: Box<dyn CurveCalculator>,
}

impl SwapCurve {
    pub fn swap(
        &self,
        source_amount: u128,
        swap_source_amount: u128,
        swap_destination_amount: u128,
        trade_direction: TradeDirection,
        fees: &Fees,
    ) -> Option<SwapResult> {
        // calc the fees
        let trade_fee = fees.trading_fee(source_amount)?; //to LPs
        let owner_fee = fees.owner_trading_fee(source_amount)?; //to owner
        let total_fees = trade_fee.checked_add(owner_fee)?; //to LPs + to owner

        // debit the fees out of the source token swap amount
        let source_amount_less_fees = source_amount.checked_sub(total_fees)?;

        // calculate the swap = CORE
        // the actual amounts that get swapped might be slightly different to requestd ones, due to how division works
        let SwapWithoutFeesResult {
            source_amount_swapped,
            destination_amount_swapped,
        } = self.calculator.swap_without_fees(
            source_amount_less_fees,
            swap_source_amount,
            swap_destination_amount,
            trade_direction,
        )?;

        // add the fees back to the source token amount
        let source_amount_swapped = source_amount_swapped.checked_add(total_fees)?;

        // return the result
        Some(SwapResult {
            new_swap_source_amount: swap_source_amount.checked_add(source_amount_swapped)?,
            new_swap_destination_amount: swap_destination_amount
                .checked_sub(destination_amount_swapped)?,
            source_amount_swapped,
            destination_amount_swapped,
            trade_fee, //todo this doesn't seem to be captured in any way?
            owner_fee,
        })
    }

    // subtracts the fee then passes down to calculate the amount of POOL tokens to withdraw
    pub fn withdraw_single_token_type_exact_out(
        &self,
        source_amount: u128, //source tokens that go to the OWNER as a fee for executing the trade
        swap_token_a_amount: u128,
        swap_token_b_amount: u128,
        pool_supply: u128,
        trade_direction: TradeDirection,
        fees: &Fees,
    ) -> Option<u128> {
        if source_amount == 0 {
            return Some(0);
        }

        // calc and sub the trading fee on half the tokens
        // todo so this is like trade fee on trade fee? why?
        let half_source_amount = std::cmp::max(1, source_amount.checked_div(2)?);
        let trade_fee = fees.trading_fee(half_source_amount)?;
        let source_amount = source_amount.checked_sub(trade_fee)?;

        self.calculator.withdraw_single_token_type_exact_out(
            source_amount, //source tokens that go to the OWNER as a fee for executing the trade LESS FEE
            swap_token_a_amount,
            swap_token_b_amount,
            pool_supply,
            trade_direction,
        )
    }
}

/// Default implementation for SwapCurve cannot be derived because of
/// the contained Box.
impl Default for SwapCurve {
    fn default() -> Self {
        let curve_type: CurveType = Default::default();
        let calculator: ConstantProductCurve = Default::default();
        Self {
            curve_type,
            calculator: Box::new(calculator),
        }
    }
}

/// Clone takes advantage of pack / unpack to get around the difficulty of
/// cloning dynamic objects.
/// Note that this is only to be used for testing.
#[cfg(any(test, feature = "fuzz"))]
impl Clone for SwapCurve {
    fn clone(&self) -> Self {
        let mut packed_self = [0u8; Self::LEN];
        Self::pack_into_slice(self, &mut packed_self);
        Self::unpack_from_slice(&packed_self).unwrap()
    }
}

/// Simple implementation for PartialEq which assumes that the output of
/// `Pack` is enough to guarantee equality
impl PartialEq for SwapCurve {
    fn eq(&self, other: &Self) -> bool {
        let mut packed_self = [0u8; Self::LEN];
        Self::pack_into_slice(self, &mut packed_self);
        let mut packed_other = [0u8; Self::LEN];
        Self::pack_into_slice(other, &mut packed_other);
        packed_self[..] == packed_other[..]
    }
}

/// Sensible default of CurveType to ConstantProduct, the most popular and
/// well-known curve type.
impl Default for CurveType {
    fn default() -> Self {
        CurveType::ConstantProduct
    }
}

impl TryFrom<u8> for CurveType {
    type Error = ProgramError;

    fn try_from(curve_type: u8) -> Result<Self, Self::Error> {
        match curve_type {
            0 => Ok(CurveType::ConstantProduct),
            // 1 => Ok(CurveType::ConstantPrice),
            // 2 => Ok(CurveType::Stable),
            // 3 => Ok(CurveType::Offset),
            _ => Err(ProgramError::InvalidAccountData),
        }
    }
}

// ----------------------------------------------------------------------------- program pack

impl Sealed for SwapCurve {}
impl Pack for SwapCurve {
    /// Size of encoding of all curve parameters, which include fees and any other
    /// constants used to calculate swaps, deposits, and withdrawals.
    /// This includes 1 byte for the type, and 72 for the calculator to use as
    /// it needs.  Some calculators may be smaller than 72 bytes.
    const LEN: usize = 33;

    /// Unpacks a byte buffer into a SwapCurve
    fn unpack_from_slice(input: &[u8]) -> Result<Self, ProgramError> {
        let input = array_ref![input, 0, 33];
        #[allow(clippy::ptr_offset_with_cast)]
        let (curve_type, calculator) = array_refs![input, 1, 32];
        let curve_type = curve_type[0].try_into()?;
        Ok(Self {
            curve_type,
            calculator: match curve_type {
                CurveType::ConstantProduct => {
                    Box::new(ConstantProductCurve::unpack_from_slice(calculator)?)
                }
                // CurveType::ConstantPrice => {
                //     Box::new(ConstantPriceCurve::unpack_from_slice(calculator)?)
                // }
                // CurveType::Stable => Box::new(StableCurve::unpack_from_slice(calculator)?),
                // CurveType::Offset => Box::new(OffsetCurve::unpack_from_slice(calculator)?),
            },
        })
    }

    /// Pack SwapCurve into a byte buffer
    fn pack_into_slice(&self, output: &mut [u8]) {
        let output = array_mut_ref![output, 0, 33];
        let (curve_type, calculator) = mut_array_refs![output, 1, 32];
        curve_type[0] = self.curve_type as u8;
        self.calculator.pack_into_slice(&mut calculator[..]);
    }
}

// ----------------------------------------------------------------------------- result

#[derive(Debug, PartialEq)]
pub struct SwapResult {
    /// New amount of source token
    pub new_swap_source_amount: u128,
    /// New amount of destination token
    pub new_swap_destination_amount: u128,
    /// Amount of source token swapped (includes fees)
    pub source_amount_swapped: u128,
    /// Amount of destination token swapped
    pub destination_amount_swapped: u128,
    /// Amount of source tokens going to pool holders
    pub trade_fee: u128,
    /// Amount of source tokens going to owner
    pub owner_fee: u128,
}
