use crate::curve::base::{CurveType, SwapCurve};
use crate::curve::fees::Fees;
use crate::error::SwapError;
use solana_program::program_error::ProgramError;

pub struct SwapConstraints<'a> {
    pub owner_key: &'a str,
    //owner of the ctr
    pub valid_curve_types: &'a [CurveType],
    pub fees: &'a Fees, //fee schedule
}

const OWNER_KEY: &str = "AFe99p6byLxYfEV9E1nNumSeKdtgXm2HL5Gy5dN6icj9";

const FEES: &Fees = &Fees {
    //.03% fee to the LPs
    trade_fee_numerator: 3,
    trade_fee_denominator: 10000,
    //.05% fee to the platform
    owner_trade_fee_numerator: 5,
    owner_trade_fee_denominator: 10000,
    owner_withdraw_fee_numerator: 0,
    owner_withdraw_fee_denominator: 0,
    // 20% of the owner fee goes to host
    host_fee_numerator: 20,
    host_fee_denominator: 100,
};

pub const SWAP_CONSTRAINTS: Option<SwapConstraints> = {
    Some(SwapConstraints {
        owner_key: OWNER_KEY,
        valid_curve_types: VALID_CURVE_TYPES,
        fees: FEES,
    })
};

const VALID_CURVE_TYPES: &[CurveType] = &[CurveType::ConstantProduct];

impl<'a> SwapConstraints<'a> {
    pub fn validate_curve(&self, swap_curve: &SwapCurve) -> Result<(), ProgramError> {
        if self
            .valid_curve_types
            .iter()
            .any(|x| *x == swap_curve.curve_type)
        {
            Ok(())
        } else {
            Err(SwapError::UnsupportedCurveType.into())
        }
    }

    pub fn validate_fees(&self, fees: &Fees) -> Result<(), ProgramError> {
        if fees.trade_fee_numerator >= self.fees.trade_fee_numerator
            && fees.trade_fee_denominator == self.fees.trade_fee_denominator
            && fees.owner_trade_fee_numerator >= self.fees.owner_trade_fee_numerator
            && fees.owner_trade_fee_denominator == self.fees.owner_trade_fee_denominator
            && fees.owner_withdraw_fee_numerator >= self.fees.owner_withdraw_fee_numerator
            && fees.owner_withdraw_fee_denominator == self.fees.owner_withdraw_fee_denominator
            && fees.host_fee_numerator == self.fees.host_fee_numerator
            && fees.host_fee_denominator == self.fees.host_fee_denominator
        {
            Ok(())
        } else {
            Err(SwapError::InvalidFee.into())
        }
    }
}
