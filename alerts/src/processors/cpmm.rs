//! Raydium CPMM (Constant Product Market Maker) instruction processor.
//!
//! This module handles decoded instructions from the Raydium CPMM program,
//! with optional filtering by token mints and AMM pool addresses.

use {
    crate::output::{
        EventType, OutputFormat, Protocol, SwapDirection, SwapEvent, TokenInfo, WebhookNotifier,
    },
    async_trait::async_trait,
    carbon_core::{
        deserialize::ArrangeAccounts, error::CarbonResult, instruction::DecodedInstruction,
        instruction::InstructionMetadata, instruction::NestedInstructions,
        metrics::MetricsCollection, processor::Processor,
    },
    carbon_raydium_cpmm_decoder::instructions::{
        deposit::Deposit, swap_base_input::SwapBaseInput, swap_base_output::SwapBaseOutput,
        withdraw::Withdraw, RaydiumCpmmInstruction,
    },
    solana_pubkey::Pubkey,
    std::{collections::HashSet, sync::Arc},
};

/// Processor for Raydium CPMM instructions with optional token and AMM filtering.
///
/// Supports filtering swaps by:
/// - Token mint addresses (input or output)
/// - AMM/pool addresses
///
/// Uses OR logic: a swap is logged if it matches ANY of the configured filters.
/// If no filters are configured, all swaps are logged.
pub struct RaydiumCpmmInstructionProcessor {
    /// Set of token mint addresses to filter. Empty means no filter (track all).
    filter_tokens: HashSet<Pubkey>,
    /// Set of AMM/pool addresses to filter. Empty means no filter (track all).
    filter_amms: HashSet<Pubkey>,
    /// Output format for swap events.
    output_format: OutputFormat,
    /// Optional webhook notifier for sending alerts.
    webhook_notifier: Option<Arc<WebhookNotifier>>,
}

impl RaydiumCpmmInstructionProcessor {
    /// Creates a new processor with optional filtering and output configuration.
    ///
    /// # Arguments
    ///
    /// * `filter_tokens` - Set of token mints to track. Empty set tracks all tokens.
    /// * `filter_amms` - Set of AMM/pool addresses to track. Empty set tracks all AMMs.
    /// * `output_format` - Format for swap event output (text, json, json_pretty).
    /// * `webhook_notifier` - Optional webhook notifier for sending alerts.
    pub fn new(
        filter_tokens: HashSet<Pubkey>,
        filter_amms: HashSet<Pubkey>,
        output_format: OutputFormat,
        webhook_notifier: Option<Arc<WebhookNotifier>>,
    ) -> Self {
        Self {
            filter_tokens,
            filter_amms,
            output_format,
            webhook_notifier,
        }
    }

    /// Checks if a swap matches any of the configured filters (OR logic).
    ///
    /// Returns `true` if:
    /// - Both filters are empty (no filtering - track all), OR
    /// - AMM matches `filter_amms`, OR
    /// - Either input or output token matches `filter_tokens`
    fn matches_filter(&self, amm: &Pubkey, input_mint: &Pubkey, output_mint: &Pubkey) -> bool {
        // If no filters configured, track everything
        if self.filter_amms.is_empty() && self.filter_tokens.is_empty() {
            return true;
        }
        // Match if AMM is in filter list
        if self.filter_amms.contains(amm) {
            return true;
        }
        // Match if either token is in filter list
        if self.filter_tokens.contains(input_mint) || self.filter_tokens.contains(output_mint) {
            return true;
        }
        false
    }

    /// Outputs a swap event and optionally sends to webhook.
    async fn emit_event(&self, event: SwapEvent) {
        // Log the event
        log::info!("{}", event.format(self.output_format));

        // Send to webhook if configured
        if let Some(ref notifier) = self.webhook_notifier {
            // Use try_send to avoid blocking the processor
            if let Err(e) = notifier.try_send(event) {
                log::warn!("Failed to queue webhook notification: {e}");
            }
        }
    }
}

#[async_trait]
impl Processor for RaydiumCpmmInstructionProcessor {
    type InputType = (
        InstructionMetadata,
        DecodedInstruction<RaydiumCpmmInstruction>,
        NestedInstructions,
        solana_instruction::Instruction,
    );

    async fn process(
        &mut self,
        (metadata, instruction, _nested_instructions, raw_instruction): Self::InputType,
        _metrics: Arc<MetricsCollection>,
    ) -> CarbonResult<()> {
        let signature = metadata.transaction_metadata.signature.to_string();
        let slot = metadata.transaction_metadata.slot;

        match instruction.data {
            // SwapBaseInput - exact input amount swap
            RaydiumCpmmInstruction::SwapBaseInput(ref swap_data) => {
                if let Some(accounts) = SwapBaseInput::arrange_accounts(&raw_instruction.accounts) {
                    if self.matches_filter(
                        &accounts.pool_state,
                        &accounts.input_token_mint,
                        &accounts.output_token_mint,
                    ) {
                        let event = SwapEvent::builder()
                            .event_type(EventType::Swap)
                            .protocol(Protocol::Cpmm)
                            .signature(&signature)
                            .pool_pubkey(&accounts.pool_state)
                            .input_token(TokenInfo::from_pubkey(
                                &accounts.input_token_mint,
                                swap_data.amount_in,
                            ))
                            .output_token(TokenInfo::from_pubkey(
                                &accounts.output_token_mint,
                                swap_data.minimum_amount_out,
                            ))
                            .direction(SwapDirection::ExactInput)
                            .maker_pubkey(&accounts.payer)
                            .slot(slot)
                            .build();

                        self.emit_event(event).await;
                    }
                }
            }
            // SwapBaseOutput - exact output amount swap
            RaydiumCpmmInstruction::SwapBaseOutput(ref swap_data) => {
                if let Some(accounts) = SwapBaseOutput::arrange_accounts(&raw_instruction.accounts)
                {
                    if self.matches_filter(
                        &accounts.pool_state,
                        &accounts.input_token_mint,
                        &accounts.output_token_mint,
                    ) {
                        let event = SwapEvent::builder()
                            .event_type(EventType::Swap)
                            .protocol(Protocol::Cpmm)
                            .signature(&signature)
                            .pool_pubkey(&accounts.pool_state)
                            .input_token(TokenInfo::from_pubkey(
                                &accounts.input_token_mint,
                                swap_data.max_amount_in,
                            ))
                            .output_token(TokenInfo::from_pubkey(
                                &accounts.output_token_mint,
                                swap_data.amount_out,
                            ))
                            .direction(SwapDirection::ExactOutput)
                            .maker_pubkey(&accounts.payer)
                            .slot(slot)
                            .build();

                        self.emit_event(event).await;
                    }
                }
            }
            // SwapEvent - contains actual amounts (not estimates)
            RaydiumCpmmInstruction::SwapEvent(ref swap_event) => {
                if self.matches_filter(
                    &swap_event.pool_id,
                    &swap_event.input_mint,
                    &swap_event.output_mint,
                ) {
                    let event = SwapEvent::builder()
                        .event_type(EventType::Swap)
                        .protocol(Protocol::Cpmm)
                        .signature(&signature)
                        .pool(swap_event.pool_id.to_string())
                        .input_token(TokenInfo::from_pubkey(
                            &swap_event.input_mint,
                            swap_event.input_amount,
                        ))
                        .output_token(TokenInfo::from_pubkey(
                            &swap_event.output_mint,
                            swap_event.output_amount,
                        ))
                        .direction(SwapDirection::Unknown)
                        .fee(swap_event.trade_fee)
                        .slot(slot)
                        .build();

                    self.emit_event(event).await;
                }
            }
            // Deposit - Add liquidity
            RaydiumCpmmInstruction::Deposit(ref deposit_data) => {
                if let Some(accounts) = Deposit::arrange_accounts(&raw_instruction.accounts) {
                    let event = SwapEvent::builder()
                        .event_type(EventType::AddLiquidity)
                        .protocol(Protocol::Cpmm)
                        .signature(&signature)
                        .pool_pubkey(&accounts.pool_state)
                        .input_token(TokenInfo::from_pubkey(
                            &accounts.vault_0_mint,
                            deposit_data.maximum_token_0_amount,
                        ))
                        .output_token(TokenInfo::from_pubkey(
                            &accounts.vault_1_mint,
                            deposit_data.maximum_token_1_amount,
                        ))
                        .maker_pubkey(&accounts.owner)
                        .slot(slot)
                        .build();

                    self.emit_event(event).await;
                }
            }
            // Withdraw - Remove liquidity
            RaydiumCpmmInstruction::Withdraw(ref withdraw_data) => {
                if let Some(accounts) = Withdraw::arrange_accounts(&raw_instruction.accounts) {
                    let event = SwapEvent::builder()
                        .event_type(EventType::RemoveLiquidity)
                        .protocol(Protocol::Cpmm)
                        .signature(&signature)
                        .pool_pubkey(&accounts.pool_state)
                        .input_token(TokenInfo::from_pubkey(
                            &accounts.vault_0_mint,
                            withdraw_data.minimum_token_0_amount,
                        ))
                        .output_token(TokenInfo::from_pubkey(
                            &accounts.vault_1_mint,
                            withdraw_data.minimum_token_1_amount,
                        ))
                        .maker_pubkey(&accounts.owner)
                        .slot(slot)
                        .build();

                    self.emit_event(event).await;
                }
            }
            // LpChangeEvent - LP change event with actual amounts
            RaydiumCpmmInstruction::LpChangeEvent(ref lp_event) => {
                // change_type: 0 = add, 1 = remove
                let event_type = if lp_event.change_type == 0 {
                    EventType::AddLiquidity
                } else {
                    EventType::RemoveLiquidity
                };

                let event = SwapEvent::builder()
                    .event_type(event_type)
                    .protocol(Protocol::Cpmm)
                    .signature(&signature)
                    .pool(lp_event.pool_id.to_string())
                    .input_token(TokenInfo::new(
                        lp_event.pool_id.to_string(), // No mint in event
                        lp_event.token_0_amount,
                    ))
                    .output_token(TokenInfo::new(
                        lp_event.pool_id.to_string(),
                        lp_event.token_1_amount,
                    ))
                    .slot(slot)
                    .build();

                self.emit_event(event).await;
            }
            // Initialize - Pool creation
            RaydiumCpmmInstruction::Initialize(ref init) => {
                log::info!(
                    "[CPMM] 🆕 Initialize: sig={}, init_amount_0={}, init_amount_1={}",
                    signature,
                    init.init_amount_0,
                    init.init_amount_1
                );
            }
            // Skip administrative events to reduce noise
            _ => {}
        };

        Ok(())
    }
}
