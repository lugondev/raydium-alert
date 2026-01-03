//! Instruction processors for Raydium DEX protocols.
//!
//! This module contains processor implementations for handling decoded instructions
//! from various Raydium AMM programs:
//!
//! - [`cpmm`] - Raydium CPMM (Constant Product Market Maker) processor
//! - [`clmm`] - Raydium CLMM (Concentrated Liquidity Market Maker) processor
//! - [`amm_v4`] - Raydium AMM V4 processor

mod amm_v4;
mod clmm;
mod cpmm;

pub use amm_v4::RaydiumAmmV4InstructionProcessor;
pub use clmm::RaydiumClmmInstructionProcessor;
pub use cpmm::RaydiumCpmmInstructionProcessor;
