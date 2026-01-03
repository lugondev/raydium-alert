//! Raydium AMM V4 instruction processor.
//!
//! This module handles decoded instructions from the Raydium AMM V4 program,
//! with optional filtering by AMM pool addresses.
//!
//! Note: AMM V4 doesn't include token mint addresses directly in instruction accounts.
//! It uses token accounts which would require on-chain lookup to get the mint.

use {
    crate::output::{
        extract_swap_amounts, EventType, OutputFormat, Protocol, SwapDirection, SwapEvent,
        TokenInfo, WebhookNotifier,
    },
    async_trait::async_trait,
    carbon_core::{
        deserialize::ArrangeAccounts, error::CarbonResult, instruction::DecodedInstruction,
        instruction::InstructionMetadata, instruction::NestedInstructions,
        metrics::MetricsCollection, processor::Processor,
    },
    carbon_raydium_amm_v4_decoder::instructions::{
        swap_base_in::SwapBaseIn, swap_base_in_v2::SwapBaseInV2, swap_base_out::SwapBaseOut,
        swap_base_out_v2::SwapBaseOutV2, RaydiumAmmV4Instruction,
    },
    solana_pubkey::Pubkey,
    std::{collections::HashSet, sync::Arc},
};

/// Processor for Raydium AMM V4 instructions with optional AMM filtering.
///
/// Supports filtering swaps by AMM/pool addresses only.
/// Token filtering is not available because AMM V4 instructions use token accounts
/// rather than mint addresses directly.
pub struct RaydiumAmmV4InstructionProcessor {
    /// Set of AMM addresses to filter. Empty means no filter (track all).
    filter_amms: HashSet<Pubkey>,
    /// Output format for swap events.
    output_format: OutputFormat,
    /// Optional webhook notifier for sending alerts.
    webhook_notifier: Option<Arc<WebhookNotifier>>,
}

impl RaydiumAmmV4InstructionProcessor {
    /// Creates a new processor with optional AMM filtering and output configuration.
    ///
    /// # Arguments
    ///
    /// * `filter_amms` - Set of AMM addresses to track. Empty set tracks all AMMs.
    /// * `output_format` - Format for swap event output (text, json, json_pretty).
    /// * `webhook_notifier` - Optional webhook notifier for sending alerts.
    pub fn new(
        filter_amms: HashSet<Pubkey>,
        output_format: OutputFormat,
        webhook_notifier: Option<Arc<WebhookNotifier>>,
    ) -> Self {
        Self {
            filter_amms,
            output_format,
            webhook_notifier,
        }
    }

    /// Checks if an AMM matches the filter.
    fn matches_amm_filter(&self, amm: &Pubkey) -> bool {
        if self.filter_amms.is_empty() {
            return true;
        }
        self.filter_amms.contains(amm)
    }

    /// Outputs a swap event and optionally sends to webhook.
    async fn emit_event(&self, event: SwapEvent) {
        log::info!("{}", event.format(self.output_format));

        if let Some(ref notifier) = self.webhook_notifier {
            if let Err(e) = notifier.try_send(event) {
                log::warn!("Failed to queue webhook notification: {e}");
            }
        }
    }
}

#[async_trait]
impl Processor for RaydiumAmmV4InstructionProcessor {
    type InputType = (
        InstructionMetadata,
        DecodedInstruction<RaydiumAmmV4Instruction>,
        NestedInstructions,
        solana_instruction::Instruction,
    );

    async fn process(
        &mut self,
        (metadata, instruction, nested_instructions, raw_instruction): Self::InputType,
        _metrics: Arc<MetricsCollection>,
    ) -> CarbonResult<()> {
        let signature = metadata.transaction_metadata.signature.to_string();
        let slot = metadata.transaction_metadata.slot;

        match instruction.data {
            // SwapBaseIn - Legacy swap with Serum
            RaydiumAmmV4Instruction::SwapBaseIn(ref swap) => {
                if let Some(accounts) = SwapBaseIn::arrange_accounts(&raw_instruction.accounts) {
                    if self.matches_amm_filter(&accounts.amm) {
                        // Extract actual amounts from nested token transfers
                        // The instruction's minimum_amount_out is just slippage protection
                        let (actual_input, actual_output) = extract_swap_amounts(
                            &nested_instructions,
                            &accounts.user_source_token_account,
                            &accounts.user_destination_token_account,
                            swap.amount_in,           // fallback to instruction amount
                            swap.minimum_amount_out,  // fallback to min (not ideal)
                        );

                        log::debug!(
                            "[AMM-V4] SwapBaseIn: sig={}, amm={}, input={} (instr={}), output={} (min={})",
                            signature,
                            accounts.amm,
                            actual_input,
                            swap.amount_in,
                            actual_output,
                            swap.minimum_amount_out
                        );

                        let event = SwapEvent::builder()
                            .event_type(EventType::Swap)
                            .protocol(Protocol::AmmV4)
                            .signature(&signature)
                            .pool_pubkey(&accounts.amm)
                            .input_token(TokenInfo::new(
                                accounts.user_source_token_account.to_string(),
                                actual_input,
                            ))
                            .output_token(TokenInfo::new(
                                accounts.user_destination_token_account.to_string(),
                                actual_output,
                            ))
                            .direction(SwapDirection::ExactInput)
                            .maker_pubkey(&accounts.user_source_owner)
                            .slot(slot)
                            .build();

                        self.emit_event(event).await;
                    }
                }
            }
            // SwapBaseOut - Legacy swap with Serum
            RaydiumAmmV4Instruction::SwapBaseOut(ref swap) => {
                if let Some(accounts) = SwapBaseOut::arrange_accounts(&raw_instruction.accounts) {
                    if self.matches_amm_filter(&accounts.amm) {
                        // Extract actual amounts from nested token transfers
                        // The instruction's max_amount_in is just slippage protection
                        let (actual_input, actual_output) = extract_swap_amounts(
                            &nested_instructions,
                            &accounts.user_source_token_account,
                            &accounts.user_destination_token_account,
                            swap.max_amount_in,  // fallback to max (not ideal)
                            swap.amount_out,     // fallback to instruction amount
                        );

                        log::debug!(
                            "[AMM-V4] SwapBaseOut: sig={}, amm={}, input={} (max={}), output={} (instr={})",
                            signature,
                            accounts.amm,
                            actual_input,
                            swap.max_amount_in,
                            actual_output,
                            swap.amount_out
                        );

                        let event = SwapEvent::builder()
                            .event_type(EventType::Swap)
                            .protocol(Protocol::AmmV4)
                            .signature(&signature)
                            .pool_pubkey(&accounts.amm)
                            .input_token(TokenInfo::new(
                                accounts.user_source_token_account.to_string(),
                                actual_input,
                            ))
                            .output_token(TokenInfo::new(
                                accounts.user_destination_token_account.to_string(),
                                actual_output,
                            ))
                            .direction(SwapDirection::ExactOutput)
                            .maker_pubkey(&accounts.user_source_owner)
                            .slot(slot)
                            .build();

                        self.emit_event(event).await;
                    }
                }
            }
            // SwapBaseInV2 - Newer swap without Serum
            RaydiumAmmV4Instruction::SwapBaseInV2(ref swap) => {
                if let Some(accounts) = SwapBaseInV2::arrange_accounts(&raw_instruction.accounts) {
                    if self.matches_amm_filter(&accounts.amm) {
                        // Extract actual amounts from nested token transfers
                        let (actual_input, actual_output) = extract_swap_amounts(
                            &nested_instructions,
                            &accounts.user_source_token_account,
                            &accounts.user_destination_token_account,
                            swap.amount_in,
                            swap.minimum_amount_out,
                        );

                        log::debug!(
                            "[AMM-V4] SwapBaseInV2: sig={}, amm={}, input={} (instr={}), output={} (min={})",
                            signature,
                            accounts.amm,
                            actual_input,
                            swap.amount_in,
                            actual_output,
                            swap.minimum_amount_out
                        );

                        let event = SwapEvent::builder()
                            .event_type(EventType::Swap)
                            .protocol(Protocol::AmmV4)
                            .signature(&signature)
                            .pool_pubkey(&accounts.amm)
                            .input_token(TokenInfo::new(
                                accounts.user_source_token_account.to_string(),
                                actual_input,
                            ))
                            .output_token(TokenInfo::new(
                                accounts.user_destination_token_account.to_string(),
                                actual_output,
                            ))
                            .direction(SwapDirection::ExactInput)
                            .maker_pubkey(&accounts.user_source_owner)
                            .slot(slot)
                            .build();

                        self.emit_event(event).await;
                    }
                }
            }
            // SwapBaseOutV2 - Newer swap without Serum
            RaydiumAmmV4Instruction::SwapBaseOutV2(ref swap) => {
                if let Some(accounts) = SwapBaseOutV2::arrange_accounts(&raw_instruction.accounts) {
                    if self.matches_amm_filter(&accounts.amm) {
                        // Extract actual amounts from nested token transfers
                        let (actual_input, actual_output) = extract_swap_amounts(
                            &nested_instructions,
                            &accounts.user_source_token_account,
                            &accounts.user_destination_token_account,
                            swap.max_amount_in,
                            swap.amount_out,
                        );

                        log::debug!(
                            "[AMM-V4] SwapBaseOutV2: sig={}, amm={}, input={} (max={}), output={} (instr={})",
                            signature,
                            accounts.amm,
                            actual_input,
                            swap.max_amount_in,
                            actual_output,
                            swap.amount_out
                        );

                        let event = SwapEvent::builder()
                            .event_type(EventType::Swap)
                            .protocol(Protocol::AmmV4)
                            .signature(&signature)
                            .pool_pubkey(&accounts.amm)
                            .input_token(TokenInfo::new(
                                accounts.user_source_token_account.to_string(),
                                actual_input,
                            ))
                            .output_token(TokenInfo::new(
                                accounts.user_destination_token_account.to_string(),
                                actual_output,
                            ))
                            .direction(SwapDirection::ExactOutput)
                            .maker_pubkey(&accounts.user_source_owner)
                            .slot(slot)
                            .build();

                        self.emit_event(event).await;
                    }
                }
            }
            // Initialize events
            RaydiumAmmV4Instruction::Initialize(ref init) => {
                log::info!(
                    "[AMM-V4] 🆕 Initialize: sig={}, nonce={}",
                    signature,
                    init.nonce
                );
            }
            RaydiumAmmV4Instruction::Initialize2(ref init) => {
                log::info!(
                    "[AMM-V4] 🆕 Initialize2: sig={}, nonce={}, open_time={}",
                    signature,
                    init.nonce,
                    init.open_time
                );
            }
            // Liquidity events
            RaydiumAmmV4Instruction::Deposit(ref deposit) => {
                log::info!(
                    "[AMM-V4] 💧 Deposit: sig={}, max_coin={}, max_pc={}, base_side={}",
                    signature,
                    deposit.max_coin_amount,
                    deposit.max_pc_amount,
                    deposit.base_side
                );
            }
            RaydiumAmmV4Instruction::Withdraw(ref withdraw) => {
                log::info!(
                    "[AMM-V4] 🔥 Withdraw: sig={}, amount={}",
                    signature,
                    withdraw.amount
                );
            }
            // Skip other events
            _ => {}
        };

        Ok(())
    }
}
