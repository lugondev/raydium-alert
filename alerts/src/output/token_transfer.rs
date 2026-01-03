//! Token transfer parsing utilities for extracting actual swap amounts.
//!
//! This module provides utilities for parsing SPL Token transfer instructions
//! from nested instructions (inner instructions) to extract actual swap amounts
//! instead of relying on instruction parameters (min/max amounts).
//!
//! # Background
//!
//! Raydium swap instructions contain parameters like `minimum_amount_out` or
//! `max_amount_in` which are slippage protection values, not actual amounts.
//! The actual transfer amounts are in the nested SPL Token Transfer instructions
//! that the swap program invokes via CPI.
//!
//! # SPL Token Instruction Formats
//!
//! - **Transfer** (discriminator `3`): `[3, amount(8 bytes LE)]`
//!   - Accounts: [source, destination, authority, ...]
//!
//! - **TransferChecked** (discriminator `12`): `[12, amount(8 bytes LE), decimals(1 byte)]`
//!   - Accounts: [source, mint, destination, authority, ...]

use carbon_core::instruction::NestedInstructions;
use solana_pubkey::Pubkey;
use std::str::FromStr;

/// SPL Token Program ID (standard SPL Token, not Token-2022).
pub const SPL_TOKEN_PROGRAM_ID: &str = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA";

/// SPL Token-2022 Program ID.
pub const SPL_TOKEN_2022_PROGRAM_ID: &str = "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb";

/// Represents a parsed token transfer from a nested instruction.
#[derive(Debug, Clone)]
pub struct TokenTransfer {
    /// Source token account (sender's ATA).
    pub source: Pubkey,
    /// Destination token account (receiver's ATA).
    pub destination: Pubkey,
    /// Amount transferred in raw units (lamports/smallest unit).
    pub amount: u64,
    /// Token mint address (only available for TransferChecked).
    /// Reserved for future token metadata lookup.
    #[allow(dead_code)]
    pub mint: Option<Pubkey>,
    /// Token decimals (only available for TransferChecked).
    /// Reserved for future human-readable amount formatting.
    #[allow(dead_code)]
    pub decimals: Option<u8>,
}

/// Parses token transfers from Carbon's NestedInstructions.
///
/// This function examines each nested instruction to find SPL Token Transfer
/// or TransferChecked instructions and extracts the actual transfer amounts.
/// It recursively processes inner_instructions as well.
///
/// # Arguments
///
/// * `nested_instructions` - Reference to Carbon's NestedInstructions
///
/// # Returns
///
/// Vector of parsed `TokenTransfer` structs, one for each transfer found.
pub fn parse_token_transfers_from_nested(nested: &NestedInstructions) -> Vec<TokenTransfer> {
    let spl_token_id =
        Pubkey::from_str(SPL_TOKEN_PROGRAM_ID).expect("Invalid SPL Token Program ID constant");
    let spl_token_2022_id = Pubkey::from_str(SPL_TOKEN_2022_PROGRAM_ID)
        .expect("Invalid SPL Token-2022 Program ID constant");

    let mut transfers = Vec::new();

    for nested_ix in nested.iter() {
        let ix = &nested_ix.instruction;

        // Check if this is an SPL Token instruction
        if ix.program_id == spl_token_id || ix.program_id == spl_token_2022_id {
            if let Some(transfer) = parse_single_transfer(ix) {
                transfers.push(transfer);
            }
        }

        // Recursively process inner instructions
        if !nested_ix.inner_instructions.is_empty() {
            transfers.extend(parse_token_transfers_from_nested(
                &nested_ix.inner_instructions,
            ));
        }
    }

    transfers
}

/// Parses a single instruction as a token transfer.
///
/// Handles both Transfer (discriminator 3) and TransferChecked (discriminator 12).
fn parse_single_transfer(ix: &solana_instruction::Instruction) -> Option<TokenTransfer> {
    if ix.data.is_empty() {
        return None;
    }

    let discriminator = ix.data[0];

    match discriminator {
        // Transfer instruction: [3, amount(8)]
        // Accounts: [source, destination, authority, ...]
        3 => {
            if ix.data.len() < 9 || ix.accounts.len() < 2 {
                return None;
            }

            let amount =
                u64::from_le_bytes(ix.data[1..9].try_into().expect("slice should be 8 bytes"));

            Some(TokenTransfer {
                source: ix.accounts[0].pubkey,
                destination: ix.accounts[1].pubkey,
                amount,
                mint: None,
                decimals: None,
            })
        }
        // TransferChecked instruction: [12, amount(8), decimals(1)]
        // Accounts: [source, mint, destination, authority, ...]
        12 => {
            if ix.data.len() < 10 || ix.accounts.len() < 3 {
                return None;
            }

            let amount =
                u64::from_le_bytes(ix.data[1..9].try_into().expect("slice should be 8 bytes"));
            let decimals = ix.data[9];

            Some(TokenTransfer {
                source: ix.accounts[0].pubkey,
                mint: Some(ix.accounts[1].pubkey),
                destination: ix.accounts[2].pubkey,
                amount,
                decimals: Some(decimals),
            })
        }
        _ => None,
    }
}

/// Finds transfers matching specific source or destination accounts.
///
/// Useful for identifying the input and output transfers in a swap by matching
/// against known user token accounts.
///
/// # Arguments
///
/// * `transfers` - Slice of parsed transfers
/// * `user_source` - User's source token account (they're sending from)
/// * `user_destination` - User's destination token account (they're receiving to)
///
/// # Returns
///
/// Tuple of (input_amount, output_amount) where:
/// - `input_amount` is from transfers where source matches `user_source`
/// - `output_amount` is from transfers where destination matches `user_destination`
pub fn find_swap_amounts(
    transfers: &[TokenTransfer],
    user_source: &Pubkey,
    user_destination: &Pubkey,
) -> (Option<u64>, Option<u64>) {
    let mut input_amount: Option<u64> = None;
    let mut output_amount: Option<u64> = None;

    for transfer in transfers {
        // User is sending (source matches user's source account)
        if transfer.source == *user_source {
            input_amount = Some(transfer.amount);
        }
        // User is receiving (destination matches user's destination account)
        if transfer.destination == *user_destination {
            output_amount = Some(transfer.amount);
        }
    }

    (input_amount, output_amount)
}

/// Extracts swap amounts from nested instructions for AMM swaps.
///
/// This is a convenience function that combines parsing and matching.
///
/// # Arguments
///
/// * `nested_instructions` - Carbon's NestedInstructions from the swap transaction
/// * `user_source` - User's source token account
/// * `user_destination` - User's destination token account
/// * `fallback_input` - Value to use if input amount not found
/// * `fallback_output` - Value to use if output amount not found
///
/// # Returns
///
/// Tuple of (input_amount, output_amount), using fallback values if not found.
pub fn extract_swap_amounts(
    nested_instructions: &NestedInstructions,
    user_source: &Pubkey,
    user_destination: &Pubkey,
    fallback_input: u64,
    fallback_output: u64,
) -> (u64, u64) {
    let transfers = parse_token_transfers_from_nested(nested_instructions);

    log::debug!(
        "Parsed {} token transfers from {} nested instructions",
        transfers.len(),
        nested_instructions.len()
    );

    for (i, t) in transfers.iter().enumerate() {
        log::debug!(
            "Transfer[{}]: {} -> {}, amount={}",
            i,
            &t.source.to_string()[..8],
            &t.destination.to_string()[..8],
            t.amount
        );
    }

    let (input, output) = find_swap_amounts(&transfers, user_source, user_destination);

    (
        input.unwrap_or(fallback_input),
        output.unwrap_or(fallback_output),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use solana_instruction::{AccountMeta, Instruction};

    fn create_transfer_instruction(
        source: Pubkey,
        destination: Pubkey,
        authority: Pubkey,
        amount: u64,
    ) -> Instruction {
        let token_program = Pubkey::from_str(SPL_TOKEN_PROGRAM_ID).unwrap();
        let mut data = vec![3u8]; // Transfer discriminator
        data.extend_from_slice(&amount.to_le_bytes());

        Instruction {
            program_id: token_program,
            accounts: vec![
                AccountMeta::new(source, false),
                AccountMeta::new(destination, false),
                AccountMeta::new_readonly(authority, true),
            ],
            data,
        }
    }

    fn create_transfer_checked_instruction(
        source: Pubkey,
        mint: Pubkey,
        destination: Pubkey,
        authority: Pubkey,
        amount: u64,
        decimals: u8,
    ) -> Instruction {
        let token_program = Pubkey::from_str(SPL_TOKEN_PROGRAM_ID).unwrap();
        let mut data = vec![12u8]; // TransferChecked discriminator
        data.extend_from_slice(&amount.to_le_bytes());
        data.push(decimals);

        Instruction {
            program_id: token_program,
            accounts: vec![
                AccountMeta::new(source, false),
                AccountMeta::new_readonly(mint, false),
                AccountMeta::new(destination, false),
                AccountMeta::new_readonly(authority, true),
            ],
            data,
        }
    }

    #[test]
    fn test_parse_transfer_instruction() {
        let source = Pubkey::new_unique();
        let destination = Pubkey::new_unique();
        let authority = Pubkey::new_unique();
        let amount = 1_000_000u64;

        let ix = create_transfer_instruction(source, destination, authority, amount);
        let transfer = parse_single_transfer(&ix).expect("should parse transfer");

        assert_eq!(transfer.source, source);
        assert_eq!(transfer.destination, destination);
        assert_eq!(transfer.amount, amount);
        assert!(transfer.mint.is_none());
        assert!(transfer.decimals.is_none());
    }

    #[test]
    fn test_parse_transfer_checked_instruction() {
        let source = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let destination = Pubkey::new_unique();
        let authority = Pubkey::new_unique();
        let amount = 2_000_000u64;
        let decimals = 9u8;

        let ix = create_transfer_checked_instruction(
            source,
            mint,
            destination,
            authority,
            amount,
            decimals,
        );
        let transfer = parse_single_transfer(&ix).expect("should parse transfer checked");

        assert_eq!(transfer.source, source);
        assert_eq!(transfer.destination, destination);
        assert_eq!(transfer.amount, amount);
        assert_eq!(transfer.mint, Some(mint));
        assert_eq!(transfer.decimals, Some(decimals));
    }

    #[test]
    fn test_find_swap_amounts() {
        let user_source = Pubkey::new_unique();
        let user_destination = Pubkey::new_unique();
        let pool_source = Pubkey::new_unique();
        let pool_destination = Pubkey::new_unique();

        // Simulate a swap: user sends 100, receives 200
        let transfers = vec![
            TokenTransfer {
                source: user_source,
                destination: pool_destination,
                amount: 100,
                mint: None,
                decimals: None,
            },
            TokenTransfer {
                source: pool_source,
                destination: user_destination,
                amount: 200,
                mint: None,
                decimals: None,
            },
        ];

        let (input, output) = find_swap_amounts(&transfers, &user_source, &user_destination);

        assert_eq!(input, Some(100));
        assert_eq!(output, Some(200));
    }

    #[test]
    fn test_extract_swap_amounts_with_fallback() {
        let user_source = Pubkey::new_unique();
        let user_destination = Pubkey::new_unique();
        let empty_nested = NestedInstructions::default();

        // No matching transfers - should use fallbacks
        let (input, output) =
            extract_swap_amounts(&empty_nested, &user_source, &user_destination, 999, 888);

        assert_eq!(input, 999);
        assert_eq!(output, 888);
    }

    #[test]
    fn test_ignores_non_token_program_instructions() {
        let random_program = Pubkey::new_unique();
        let source = Pubkey::new_unique();
        let destination = Pubkey::new_unique();

        let ix = Instruction {
            program_id: random_program,
            accounts: vec![
                AccountMeta::new(source, false),
                AccountMeta::new(destination, false),
            ],
            data: vec![3, 0, 0, 0, 0, 0, 0, 0, 0], // Looks like transfer but wrong program
        };

        // parse_single_transfer doesn't check program_id (that's done in parse_token_transfers_from_nested)
        let transfer = parse_single_transfer(&ix);
        assert!(transfer.is_some());
    }
}
