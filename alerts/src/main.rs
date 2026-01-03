use {
    async_trait::async_trait,
    carbon_core::{
        deserialize::ArrangeAccounts,
        error::CarbonResult,
        instruction::{DecodedInstruction, InstructionMetadata, NestedInstructions},
        metrics::MetricsCollection,
        processor::Processor,
    },
    carbon_log_metrics::LogMetrics,
    carbon_raydium_amm_v4_decoder::{
        instructions::RaydiumAmmV4Instruction, RaydiumAmmV4Decoder,
        PROGRAM_ID as RAYDIUM_AMM_V4_PROGRAM_ID,
    },
    carbon_raydium_cpmm_decoder::{
        instructions::{
            swap_base_input::SwapBaseInput as CpmmSwapBaseInput,
            swap_base_output::SwapBaseOutput as CpmmSwapBaseOutput, RaydiumCpmmInstruction,
        },
        RaydiumCpmmDecoder, PROGRAM_ID as RAYDIUM_CPMM_PROGRAM_ID,
    },
    carbon_rpc_block_subscribe_datasource::{Filters, RpcBlockSubscribe},
    solana_client::rpc_config::{RpcBlockSubscribeConfig, RpcBlockSubscribeFilter},
    solana_pubkey::Pubkey,
    std::{collections::HashSet, env, str::FromStr, sync::Arc},
};

/// Parses a comma-separated list of pubkey addresses from an environment variable.
///
/// # Arguments
///
/// * `env_var` - The name of the environment variable to read
///
/// # Returns
///
/// A `HashSet` of `Pubkey` addresses. Returns empty set if the env var is not set or empty.
fn parse_pubkey_filter(env_var: &str) -> HashSet<Pubkey> {
    env::var(env_var)
        .ok()
        .map(|val| {
            val.split(',')
                .filter_map(|s| {
                    let trimmed = s.trim();
                    if trimmed.is_empty() {
                        return None;
                    }
                    match Pubkey::from_str(trimmed) {
                        Ok(pk) => Some(pk),
                        Err(e) => {
                            log::warn!("Invalid pubkey '{}' in {}: {}", trimmed, env_var, e);
                            None
                        }
                    }
                })
                .collect()
        })
        .unwrap_or_default()
}

#[tokio::main]
pub async fn main() -> CarbonResult<()> {
    dotenv::dotenv().ok();
    env_logger::init();

    // Create filter for both CPMM and AMM V4 programs
    // Note: RpcBlockSubscribeFilter only supports single program, so we use "All" and filter in processor
    let filters = Filters::new(
        RpcBlockSubscribeFilter::All,
        Some(RpcBlockSubscribeConfig {
            max_supported_transaction_version: Some(0),
            ..RpcBlockSubscribeConfig::default()
        }),
    );

    let rpc_ws_url =
        env::var("RPC_WS_URL").unwrap_or_else(|_| "wss://api.mainnet-beta.solana.com/".to_string());

    // Parse filters from environment variables
    // Example: FILTER_TOKENS=So11111111111111111111111111111111111111112,EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v
    // Example: FILTER_AMMS=zcdAw3jpcqEY8JYVxNVMqs2cU35cyDdy4ot7V8edNhz,CaysL4cjU1BuB9ECvhQ4yNQBVt7eug3GcZjndcJdf5JU
    let filter_tokens = parse_pubkey_filter("FILTER_TOKENS");
    let filter_amms = parse_pubkey_filter("FILTER_AMMS");

    log::info!("Raydium CPMM Program ID: {}", RAYDIUM_CPMM_PROGRAM_ID);
    log::info!("Raydium AMM V4 Program ID: {}", RAYDIUM_AMM_V4_PROGRAM_ID);

    // Log token filter status
    if filter_tokens.is_empty() {
        log::info!("Token filter: disabled (tracking all tokens)");
    } else {
        log::info!(
            "Token filter: {} token(s) - {:?}",
            filter_tokens.len(),
            filter_tokens
        );
    }

    // Log AMM filter status
    if filter_amms.is_empty() {
        log::info!("AMM filter: disabled (tracking all AMMs)");
    } else {
        log::info!(
            "AMM filter: {} AMM(s) - {:?}",
            filter_amms.len(),
            filter_amms
        );
    }

    log::info!("Starting with RPC: {rpc_ws_url}");

    let block_subscribe = RpcBlockSubscribe::new(rpc_ws_url, filters);

    // Create the processors with filters
    let cpmm_processor =
        RaydiumCpmmInstructionProcessor::new(filter_tokens.clone(), filter_amms.clone());
    let amm_v4_processor = RaydiumAmmV4InstructionProcessor::new(filter_amms);

    carbon_core::pipeline::Pipeline::builder()
        .datasource(block_subscribe)
        .metrics(Arc::new(LogMetrics::new()))
        .metrics_flush_interval(3)
        // Add both CPMM and AMM V4 decoders
        .instruction(RaydiumCpmmDecoder, cpmm_processor)
        .instruction(RaydiumAmmV4Decoder, amm_v4_processor)
        .shutdown_strategy(carbon_core::pipeline::ShutdownStrategy::Immediate)
        .build()?
        .run()
        .await?;

    Ok(())
}

// =============================================================================
// CPMM Processor
// =============================================================================

/// Processor for Raydium CPMM instructions with optional token and AMM filtering.
pub struct RaydiumCpmmInstructionProcessor {
    /// Set of token mint addresses to filter. Empty means no filter (track all).
    filter_tokens: HashSet<Pubkey>,
    /// Set of AMM/pool addresses to filter. Empty means no filter (track all).
    filter_amms: HashSet<Pubkey>,
}

impl RaydiumCpmmInstructionProcessor {
    /// Creates a new processor with optional filtering.
    ///
    /// # Arguments
    ///
    /// * `filter_tokens` - Set of token mints to track. Empty set tracks all tokens.
    /// * `filter_amms` - Set of AMM/pool addresses to track. Empty set tracks all AMMs.
    pub fn new(filter_tokens: HashSet<Pubkey>, filter_amms: HashSet<Pubkey>) -> Self {
        Self {
            filter_tokens,
            filter_amms,
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
        let signature = metadata.transaction_metadata.signature;

        match instruction.data {
            // Filter SwapBaseInput by token mint or pool (OR logic)
            RaydiumCpmmInstruction::SwapBaseInput(ref swap_base_input) => {
                if let Some(accounts) =
                    CpmmSwapBaseInput::arrange_accounts(&raw_instruction.accounts)
                {
                    if self.matches_filter(
                        &accounts.pool_state,
                        &accounts.input_token_mint,
                        &accounts.output_token_mint,
                    ) {
                        log::info!(
                            "[CPMM] SwapBaseInput: sig={signature}, pool={}, \
                            in={}, out={}, \
                            amount_in={}, min_out={}",
                            accounts.pool_state,
                            accounts.input_token_mint,
                            accounts.output_token_mint,
                            swap_base_input.amount_in,
                            swap_base_input.minimum_amount_out
                        );
                    }
                }
            }
            // Filter SwapBaseOutput by token mint or pool (OR logic)
            RaydiumCpmmInstruction::SwapBaseOutput(ref swap_base_output) => {
                if let Some(accounts) =
                    CpmmSwapBaseOutput::arrange_accounts(&raw_instruction.accounts)
                {
                    if self.matches_filter(
                        &accounts.pool_state,
                        &accounts.input_token_mint,
                        &accounts.output_token_mint,
                    ) {
                        log::info!(
                            "[CPMM] SwapBaseOutput: sig={signature}, pool={}, \
                            in={}, out={}, \
                            max_in={}, amount_out={}",
                            accounts.pool_state,
                            accounts.input_token_mint,
                            accounts.output_token_mint,
                            swap_base_output.max_amount_in,
                            swap_base_output.amount_out
                        );
                    }
                }
            }
            // Filter SwapEvent by token mint or pool (OR logic)
            RaydiumCpmmInstruction::SwapEvent(ref swap_event) => {
                if self.matches_filter(
                    &swap_event.pool_id,
                    &swap_event.input_mint,
                    &swap_event.output_mint,
                ) {
                    log::info!(
                        "[CPMM] SwapEvent: sig={signature}, \
                        pool={}, in={}, out={}, \
                        in_amt={}, out_amt={}, fee={}",
                        swap_event.pool_id,
                        swap_event.input_mint,
                        swap_event.output_mint,
                        swap_event.input_amount,
                        swap_event.output_amount,
                        swap_event.trade_fee
                    );
                }
            }
            // Log other important events without filtering
            RaydiumCpmmInstruction::Initialize(ref init) => {
                log::info!("[CPMM] Initialize: sig={signature}, init={init:?}");
            }
            RaydiumCpmmInstruction::Deposit(ref deposit) => {
                log::info!("[CPMM] Deposit: sig={signature}, deposit={deposit:?}");
            }
            RaydiumCpmmInstruction::Withdraw(ref withdraw) => {
                log::info!("[CPMM] Withdraw: sig={signature}, withdraw={withdraw:?}");
            }
            RaydiumCpmmInstruction::LpChangeEvent(ref lp_change) => {
                log::info!("[CPMM] LpChangeEvent: sig={signature}, lp_change={lp_change:?}");
            }
            // Skip administrative events to reduce noise
            _ => {}
        };

        Ok(())
    }
}

// =============================================================================
// AMM V4 Processor
// =============================================================================

/// Processor for Raydium AMM V4 instructions with optional AMM filtering.
///
/// Note: AMM V4 doesn't include token mint addresses directly in instruction accounts.
/// It uses token accounts (user_source_token_account, user_destination_token_account)
/// which would require on-chain lookup to get the mint. Only AMM address filtering is supported.
pub struct RaydiumAmmV4InstructionProcessor {
    /// Set of AMM addresses to filter. Empty means no filter (track all).
    filter_amms: HashSet<Pubkey>,
}

impl RaydiumAmmV4InstructionProcessor {
    /// Creates a new processor with optional AMM filtering.
    ///
    /// # Arguments
    ///
    /// * `filter_amms` - Set of AMM addresses to track. Empty set tracks all AMMs.
    pub fn new(filter_amms: HashSet<Pubkey>) -> Self {
        Self { filter_amms }
    }

    /// Checks if an AMM matches the filter.
    ///
    /// Returns `true` if:
    /// - No filter is set (empty set), OR
    /// - The AMM address matches a filtered AMM
    fn matches_amm_filter(&self, amm: &Pubkey) -> bool {
        if self.filter_amms.is_empty() {
            return true;
        }
        self.filter_amms.contains(amm)
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
        (metadata, instruction, _nested_instructions, raw_instruction): Self::InputType,
        _metrics: Arc<MetricsCollection>,
    ) -> CarbonResult<()> {
        use carbon_raydium_amm_v4_decoder::instructions::{
            swap_base_in::SwapBaseIn, swap_base_in_v2::SwapBaseInV2, swap_base_out::SwapBaseOut,
            swap_base_out_v2::SwapBaseOutV2,
        };

        let signature = metadata.transaction_metadata.signature;

        match instruction.data {
            // SwapBaseIn - Legacy swap with Serum integration
            RaydiumAmmV4Instruction::SwapBaseIn(ref swap) => {
                if let Some(accounts) = SwapBaseIn::arrange_accounts(&raw_instruction.accounts) {
                    if self.matches_amm_filter(&accounts.amm) {
                        log::info!(
                            "[AMM-V4] SwapBaseIn: sig={signature}, \
                            amm={}, amount_in={}, min_out={}, \
                            src={}, dst={}",
                            accounts.amm,
                            swap.amount_in,
                            swap.minimum_amount_out,
                            accounts.user_source_token_account,
                            accounts.user_destination_token_account
                        );
                    }
                }
            }
            // SwapBaseOut - Legacy swap with Serum integration
            RaydiumAmmV4Instruction::SwapBaseOut(ref swap) => {
                if let Some(accounts) = SwapBaseOut::arrange_accounts(&raw_instruction.accounts) {
                    if self.matches_amm_filter(&accounts.amm) {
                        log::info!(
                            "[AMM-V4] SwapBaseOut: sig={signature}, \
                            amm={}, max_in={}, amount_out={}, \
                            src={}, dst={}",
                            accounts.amm,
                            swap.max_amount_in,
                            swap.amount_out,
                            accounts.user_source_token_account,
                            accounts.user_destination_token_account
                        );
                    }
                }
            }
            // SwapBaseInV2 - Newer swap without Serum
            RaydiumAmmV4Instruction::SwapBaseInV2(ref swap) => {
                if let Some(accounts) = SwapBaseInV2::arrange_accounts(&raw_instruction.accounts) {
                    if self.matches_amm_filter(&accounts.amm) {
                        log::info!(
                            "[AMM-V4] SwapBaseInV2: sig={signature}, \
                            amm={}, amount_in={}, min_out={}, \
                            src={}, dst={}",
                            accounts.amm,
                            swap.amount_in,
                            swap.minimum_amount_out,
                            accounts.user_source_token_account,
                            accounts.user_destination_token_account
                        );
                    }
                }
            }
            // SwapBaseOutV2 - Newer swap without Serum
            RaydiumAmmV4Instruction::SwapBaseOutV2(ref swap) => {
                if let Some(accounts) = SwapBaseOutV2::arrange_accounts(&raw_instruction.accounts) {
                    if self.matches_amm_filter(&accounts.amm) {
                        log::info!(
                            "[AMM-V4] SwapBaseOutV2: sig={signature}, \
                            amm={}, max_in={}, amount_out={}, \
                            src={}, dst={}",
                            accounts.amm,
                            swap.max_amount_in,
                            swap.amount_out,
                            accounts.user_source_token_account,
                            accounts.user_destination_token_account
                        );
                    }
                }
            }
            // Initialize events
            RaydiumAmmV4Instruction::Initialize(ref init) => {
                log::info!("[AMM-V4] Initialize: sig={signature}, init={init:?}");
            }
            RaydiumAmmV4Instruction::Initialize2(ref init) => {
                log::info!("[AMM-V4] Initialize2: sig={signature}, init={init:?}");
            }
            // Liquidity events
            RaydiumAmmV4Instruction::Deposit(ref deposit) => {
                log::info!("[AMM-V4] Deposit: sig={signature}, deposit={deposit:?}");
            }
            RaydiumAmmV4Instruction::Withdraw(ref withdraw) => {
                log::info!("[AMM-V4] Withdraw: sig={signature}, withdraw={withdraw:?}");
            }
            // Skip administrative events to reduce noise
            _ => {}
        };

        Ok(())
    }
}
