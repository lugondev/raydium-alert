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

/// Parses a comma-separated list of token mint addresses from an environment variable.
///
/// # Arguments
///
/// * `env_var` - The name of the environment variable to read
///
/// # Returns
///
/// A `HashSet` of `Pubkey` addresses. Returns empty set if the env var is not set.
fn parse_token_filter(env_var: &str) -> HashSet<Pubkey> {
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
                            log::warn!("Invalid pubkey '{}': {}", trimmed, e);
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

    // Parse token filter from environment variable
    // Example: FILTER_TOKENS=So11111111111111111111111111111111111111112,EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v
    let filter_tokens = parse_token_filter("FILTER_TOKENS");

    log::info!("Raydium CPMM Program ID: {}", RAYDIUM_CPMM_PROGRAM_ID);
    log::info!("Raydium AMM V4 Program ID: {}", RAYDIUM_AMM_V4_PROGRAM_ID);

    if filter_tokens.is_empty() {
        log::info!("Starting with RPC: {rpc_ws_url} (no token filter - tracking all tokens)");
    } else {
        log::info!(
            "Starting with RPC: {rpc_ws_url} (filtering {} token(s): {:?})",
            filter_tokens.len(),
            filter_tokens
        );
    }

    let block_subscribe = RpcBlockSubscribe::new(rpc_ws_url, filters);

    // Create the processors with the token filter
    let cpmm_processor = RaydiumCpmmInstructionProcessor::new(filter_tokens.clone());
    let amm_v4_processor = RaydiumAmmV4InstructionProcessor::new(filter_tokens);

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

/// Processor for Raydium CPMM instructions with optional token filtering.
pub struct RaydiumCpmmInstructionProcessor {
    /// Set of token mint addresses to filter. Empty means no filter (track all).
    filter_tokens: HashSet<Pubkey>,
}

impl RaydiumCpmmInstructionProcessor {
    /// Creates a new processor with optional token filtering.
    ///
    /// # Arguments
    ///
    /// * `filter_tokens` - Set of token mints to track. Empty set tracks all tokens.
    pub fn new(filter_tokens: HashSet<Pubkey>) -> Self {
        Self { filter_tokens }
    }

    /// Checks if a swap involves any of the filtered tokens.
    ///
    /// Returns `true` if:
    /// - No filter is set (empty set), OR
    /// - Either input or output token matches a filtered token
    fn matches_filter(&self, input_mint: &Pubkey, output_mint: &Pubkey) -> bool {
        if self.filter_tokens.is_empty() {
            return true;
        }
        self.filter_tokens.contains(input_mint) || self.filter_tokens.contains(output_mint)
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
            // Filter SwapBaseInput by token mint
            RaydiumCpmmInstruction::SwapBaseInput(ref swap_base_input) => {
                if let Some(accounts) =
                    CpmmSwapBaseInput::arrange_accounts(&raw_instruction.accounts)
                {
                    if self.matches_filter(&accounts.input_token_mint, &accounts.output_token_mint) {
                        log::info!(
                            "[CPMM] SwapBaseInput: sig={signature}, \
                            in={}, out={}, \
                            amount_in={}, min_out={}",
                            accounts.input_token_mint,
                            accounts.output_token_mint,
                            swap_base_input.amount_in,
                            swap_base_input.minimum_amount_out
                        );
                    }
                }
            }
            // Filter SwapBaseOutput by token mint
            RaydiumCpmmInstruction::SwapBaseOutput(ref swap_base_output) => {
                if let Some(accounts) =
                    CpmmSwapBaseOutput::arrange_accounts(&raw_instruction.accounts)
                {
                    if self.matches_filter(&accounts.input_token_mint, &accounts.output_token_mint) {
                        log::info!(
                            "[CPMM] SwapBaseOutput: sig={signature}, \
                            in={}, out={}, \
                            max_in={}, amount_out={}",
                            accounts.input_token_mint,
                            accounts.output_token_mint,
                            swap_base_output.max_amount_in,
                            swap_base_output.amount_out
                        );
                    }
                }
            }
            // Filter SwapEvent by token mint (contains mint info directly)
            RaydiumCpmmInstruction::SwapEvent(ref swap_event) => {
                if self.matches_filter(&swap_event.input_mint, &swap_event.output_mint) {
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

/// Processor for Raydium AMM V4 instructions.
///
/// Note: AMM V4 doesn't include token mint addresses directly in instruction accounts.
/// It uses token accounts (user_source_token_account, user_destination_token_account)
/// which would require on-chain lookup to get the mint. For now, we log all swaps.
pub struct RaydiumAmmV4InstructionProcessor {
    /// Set of token mint addresses to filter. Empty means no filter (track all).
    /// Note: Token filtering for AMM V4 requires on-chain lookup (not implemented).
    #[allow(dead_code)]
    filter_tokens: HashSet<Pubkey>,
}

impl RaydiumAmmV4InstructionProcessor {
    /// Creates a new processor.
    ///
    /// # Arguments
    ///
    /// * `filter_tokens` - Set of token mints to track (reserved for future use).
    pub fn new(filter_tokens: HashSet<Pubkey>) -> Self {
        Self { filter_tokens }
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
            swap_base_in::SwapBaseIn, swap_base_in_v2::SwapBaseInV2,
            swap_base_out::SwapBaseOut, swap_base_out_v2::SwapBaseOutV2,
        };

        let signature = metadata.transaction_metadata.signature;

        match instruction.data {
            // SwapBaseIn - Legacy swap with Serum integration
            RaydiumAmmV4Instruction::SwapBaseIn(ref swap) => {
                if let Some(accounts) = SwapBaseIn::arrange_accounts(&raw_instruction.accounts) {
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
            // SwapBaseOut - Legacy swap with Serum integration
            RaydiumAmmV4Instruction::SwapBaseOut(ref swap) => {
                if let Some(accounts) = SwapBaseOut::arrange_accounts(&raw_instruction.accounts) {
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
            // SwapBaseInV2 - Newer swap without Serum
            RaydiumAmmV4Instruction::SwapBaseInV2(ref swap) => {
                if let Some(accounts) = SwapBaseInV2::arrange_accounts(&raw_instruction.accounts) {
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
            // SwapBaseOutV2 - Newer swap without Serum
            RaydiumAmmV4Instruction::SwapBaseOutV2(ref swap) => {
                if let Some(accounts) = SwapBaseOutV2::arrange_accounts(&raw_instruction.accounts) {
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
