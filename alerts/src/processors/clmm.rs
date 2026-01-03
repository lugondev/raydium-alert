//! Raydium CLMM (Concentrated Liquidity Market Maker) instruction processor.
//!
//! This module handles decoded instructions from the Raydium CLMM program,
//! with optional filtering by token mints and pool addresses.
//!
//! CLMM is a concentrated liquidity AMM similar to Uniswap V3, offering
//! more capital-efficient liquidity positions.

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
    carbon_raydium_clmm_decoder::instructions::{
        create_pool::CreatePool, swap::Swap, swap_v2::SwapV2, RaydiumClmmInstruction,
    },
    solana_pubkey::Pubkey,
    std::{collections::HashSet, sync::Arc},
};

/// Processor for Raydium CLMM instructions with optional token and pool filtering.
///
/// Supports filtering swaps by:
/// - Token mint addresses (input or output) - only available for SwapV2
/// - Pool addresses
///
/// Uses OR logic: a swap is logged if it matches ANY of the configured filters.
/// If no filters are configured, all swaps are logged.
pub struct RaydiumClmmInstructionProcessor {
    /// Set of token mint addresses to filter. Empty means no filter (track all).
    filter_tokens: HashSet<Pubkey>,
    /// Set of pool addresses to filter. Empty means no filter (track all).
    filter_pools: HashSet<Pubkey>,
    /// Output format for swap events.
    output_format: OutputFormat,
    /// Optional webhook notifier for sending alerts.
    webhook_notifier: Option<Arc<WebhookNotifier>>,
}

impl RaydiumClmmInstructionProcessor {
    /// Creates a new processor with optional filtering and output configuration.
    ///
    /// # Arguments
    ///
    /// * `filter_tokens` - Set of token mints to track. Empty set tracks all tokens.
    /// * `filter_pools` - Set of pool addresses to track. Empty set tracks all pools.
    /// * `output_format` - Format for swap event output (text, json, json_pretty).
    /// * `webhook_notifier` - Optional webhook notifier for sending alerts.
    pub fn new(
        filter_tokens: HashSet<Pubkey>,
        filter_pools: HashSet<Pubkey>,
        output_format: OutputFormat,
        webhook_notifier: Option<Arc<WebhookNotifier>>,
    ) -> Self {
        Self {
            filter_tokens,
            filter_pools,
            output_format,
            webhook_notifier,
        }
    }

    /// Checks if a swap matches any of the configured filters (OR logic).
    fn matches_filter(
        &self,
        pool: &Pubkey,
        input_mint: Option<&Pubkey>,
        output_mint: Option<&Pubkey>,
    ) -> bool {
        // If no filters configured, track everything
        if self.filter_pools.is_empty() && self.filter_tokens.is_empty() {
            return true;
        }
        // Match if pool is in filter list
        if self.filter_pools.contains(pool) {
            return true;
        }
        // Match if either token is in filter list
        if let Some(input) = input_mint {
            if self.filter_tokens.contains(input) {
                return true;
            }
        }
        if let Some(output) = output_mint {
            if self.filter_tokens.contains(output) {
                return true;
            }
        }
        false
    }

    /// Checks if a pool matches the filter (for instructions without token mints).
    fn matches_pool_filter(&self, pool: &Pubkey) -> bool {
        if self.filter_pools.is_empty() && self.filter_tokens.is_empty() {
            return true;
        }
        // When we don't have token info, only match by pool
        if self.filter_pools.is_empty() {
            return true;
        }
        self.filter_pools.contains(pool)
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
impl Processor for RaydiumClmmInstructionProcessor {
    type InputType = (
        InstructionMetadata,
        DecodedInstruction<RaydiumClmmInstruction>,
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
            // Legacy Swap - doesn't include token mints
            RaydiumClmmInstruction::Swap(ref swap) => {
                if let Some(accounts) = Swap::arrange_accounts(&raw_instruction.accounts) {
                    if self.matches_pool_filter(&accounts.pool_state) {
                        let direction = if swap.is_base_input {
                            SwapDirection::ExactInput
                        } else {
                            SwapDirection::ExactOutput
                        };

                        let (input_amount, output_amount) = if swap.is_base_input {
                            (swap.amount, swap.other_amount_threshold)
                        } else {
                            (swap.other_amount_threshold, swap.amount)
                        };

                        let event = SwapEvent::builder()
                            .event_type(EventType::Swap)
                            .protocol(Protocol::Clmm)
                            .signature(&signature)
                            .pool_pubkey(&accounts.pool_state)
                            .input_token(TokenInfo::new(
                                accounts.pool_state.to_string(), // No mint available
                                input_amount,
                            ))
                            .output_token(TokenInfo::new(
                                accounts.pool_state.to_string(),
                                output_amount,
                            ))
                            .direction(direction)
                            .maker_pubkey(&accounts.payer)
                            .slot(slot)
                            .build();

                        self.emit_event(event).await;
                    }
                }
            }
            // SwapV2 - includes token mints
            RaydiumClmmInstruction::SwapV2(ref swap) => {
                if let Some(accounts) = SwapV2::arrange_accounts(&raw_instruction.accounts) {
                    if self.matches_filter(
                        &accounts.pool_state,
                        Some(&accounts.input_vault_mint),
                        Some(&accounts.output_vault_mint),
                    ) {
                        let direction = if swap.is_base_input {
                            SwapDirection::ExactInput
                        } else {
                            SwapDirection::ExactOutput
                        };

                        let (input_amount, output_amount) = if swap.is_base_input {
                            (swap.amount, swap.other_amount_threshold)
                        } else {
                            (swap.other_amount_threshold, swap.amount)
                        };

                        let event = SwapEvent::builder()
                            .event_type(EventType::Swap)
                            .protocol(Protocol::Clmm)
                            .signature(&signature)
                            .pool_pubkey(&accounts.pool_state)
                            .input_token(TokenInfo::from_pubkey(
                                &accounts.input_vault_mint,
                                input_amount,
                            ))
                            .output_token(TokenInfo::from_pubkey(
                                &accounts.output_vault_mint,
                                output_amount,
                            ))
                            .direction(direction)
                            .maker_pubkey(&accounts.payer)
                            .slot(slot)
                            .build();

                        self.emit_event(event).await;
                    }
                }
            }
            // SwapEvent - actual amounts
            RaydiumClmmInstruction::SwapEvent(ref swap_event) => {
                if self.matches_pool_filter(&swap_event.pool_state) {
                    let (input_amount, output_amount) = if swap_event.zero_for_one {
                        (swap_event.amount0, swap_event.amount1)
                    } else {
                        (swap_event.amount1, swap_event.amount0)
                    };

                    let event = SwapEvent::builder()
                        .event_type(EventType::Swap)
                        .protocol(Protocol::Clmm)
                        .signature(&signature)
                        .pool(swap_event.pool_state.to_string())
                        .input_token(TokenInfo::new(
                            swap_event.token_account0.to_string(),
                            input_amount,
                        ))
                        .output_token(TokenInfo::new(
                            swap_event.token_account1.to_string(),
                            output_amount,
                        ))
                        .direction(SwapDirection::Unknown)
                        .maker_pubkey(&swap_event.sender)
                        .slot(slot)
                        .build();

                    self.emit_event(event).await;
                }
            }
            // CreatePool
            RaydiumClmmInstruction::CreatePool(ref create_pool) => {
                if let Some(accounts) = CreatePool::arrange_accounts(&raw_instruction.accounts) {
                    let event = SwapEvent::builder()
                        .event_type(EventType::CreatePool)
                        .protocol(Protocol::Clmm)
                        .signature(&signature)
                        .pool_pubkey(&accounts.pool_state)
                        .input_token(TokenInfo::from_pubkey(&accounts.token_mint0, 0))
                        .output_token(TokenInfo::from_pubkey(&accounts.token_mint1, 0))
                        .maker_pubkey(&accounts.pool_creator)
                        .slot(slot)
                        .build();

                    log::info!(
                        "[CLMM] 🆕 CreatePool: sqrt_price={}, open_time={}",
                        create_pool.sqrt_price_x64,
                        create_pool.open_time
                    );
                    self.emit_event(event).await;
                }
            }
            // PoolCreatedEvent
            RaydiumClmmInstruction::PoolCreatedEvent(ref event) => {
                log::info!(
                    "[CLMM] 🆕 PoolCreatedEvent: sig={}, pool={}, tick_spacing={}, sqrt_price={}",
                    signature,
                    event.pool_state,
                    event.tick_spacing,
                    event.sqrt_price_x64
                );
            }
            // Liquidity events
            RaydiumClmmInstruction::IncreaseLiquidity(ref liq) => {
                log::info!(
                    "[CLMM] 💧 IncreaseLiquidity: sig={}, liquidity={}, amount0_max={}, amount1_max={}",
                    signature,
                    liq.liquidity,
                    liq.amount0_max,
                    liq.amount1_max
                );
            }
            RaydiumClmmInstruction::IncreaseLiquidityV2(ref liq) => {
                log::info!(
                    "[CLMM] 💧 IncreaseLiquidityV2: sig={}, liquidity={}, amount0_max={}, amount1_max={}",
                    signature,
                    liq.liquidity,
                    liq.amount0_max,
                    liq.amount1_max
                );
            }
            RaydiumClmmInstruction::DecreaseLiquidity(ref liq) => {
                log::info!(
                    "[CLMM] 🔥 DecreaseLiquidity: sig={}, liquidity={}, amount0_min={}, amount1_min={}",
                    signature,
                    liq.liquidity,
                    liq.amount0_min,
                    liq.amount1_min
                );
            }
            RaydiumClmmInstruction::DecreaseLiquidityV2(ref liq) => {
                log::info!(
                    "[CLMM] 🔥 DecreaseLiquidityV2: sig={}, liquidity={}, amount0_min={}, amount1_min={}",
                    signature,
                    liq.liquidity,
                    liq.amount0_min,
                    liq.amount1_min
                );
            }
            RaydiumClmmInstruction::LiquidityChangeEvent(ref event) => {
                // Determine direction based on liquidity change
                let event_type = if event.liquidity_after > event.liquidity_before {
                    EventType::AddLiquidity
                } else {
                    EventType::RemoveLiquidity
                };
                let liquidity_delta = event.liquidity_after.abs_diff(event.liquidity_before);

                log::info!(
                    "[CLMM] {} LiquidityChangeEvent: sig={}, pool={}, liquidity_delta={}, tick={}",
                    if event_type == EventType::AddLiquidity { "💧" } else { "🔥" },
                    signature,
                    event.pool_state,
                    liquidity_delta,
                    event.tick
                );
            }
            // Position events
            RaydiumClmmInstruction::OpenPosition(ref pos) => {
                log::info!(
                    "[CLMM] 📍 OpenPosition: sig={}, tick_lower={}, tick_upper={}",
                    signature,
                    pos.tick_lower_index,
                    pos.tick_upper_index
                );
            }
            RaydiumClmmInstruction::OpenPositionV2(ref pos) => {
                log::info!(
                    "[CLMM] 📍 OpenPositionV2: sig={}, tick_lower={}, tick_upper={}",
                    signature,
                    pos.tick_lower_index,
                    pos.tick_upper_index
                );
            }
            RaydiumClmmInstruction::ClosePosition(_) => {
                log::info!("[CLMM] ❌ ClosePosition: sig={}", signature);
            }
            // Skip other events
            _ => {}
        };

        Ok(())
    }
}
