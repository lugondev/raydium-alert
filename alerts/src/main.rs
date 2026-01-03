//! Raydium DEX Alert System
//!
//! This application monitors Raydium swap events on the Solana blockchain in real-time.
//! It supports CPMM, CLMM, and AMM V4 programs with configurable filtering.
//!
//! # Configuration
//!
//! Environment variables:
//! - `RPC_WS_URL` - WebSocket RPC endpoint (default: wss://api.mainnet-beta.solana.com/)
//! - `FILTER_MARKETS` - Comma-separated list of markets to listen: cpmm, clmm, amm_v4 (default: all)
//! - `FILTER_TOKENS` - Comma-separated list of token mints to filter (optional)
//! - `FILTER_AMMS` - Comma-separated list of AMM/pool addresses to filter (optional)
//! - `OUTPUT_FORMAT` - Output format: text, json, json_pretty (default: text)
//! - `WEBHOOK_URL` - Optional webhook URL for notifications
//!
//! # Example
//!
//! ```bash
//! export RPC_WS_URL="wss://your-rpc-endpoint.com"
//! export FILTER_MARKETS="clmm,cpmm"  # Only listen to CLMM and CPMM
//! export FILTER_TOKENS="So11111111111111111111111111111111111111112"
//! export OUTPUT_FORMAT="json"
//! cargo run
//! ```

mod config;
mod output;
mod processors;

use {
    carbon_core::{error::CarbonResult, pipeline::Pipeline},
    carbon_log_metrics::LogMetrics,
    carbon_raydium_amm_v4_decoder::{RaydiumAmmV4Decoder, PROGRAM_ID as RAYDIUM_AMM_V4_PROGRAM_ID},
    carbon_raydium_clmm_decoder::{RaydiumClmmDecoder, PROGRAM_ID as RAYDIUM_CLMM_PROGRAM_ID},
    carbon_raydium_cpmm_decoder::{RaydiumCpmmDecoder, PROGRAM_ID as RAYDIUM_CPMM_PROGRAM_ID},
    carbon_rpc_block_subscribe_datasource::{Filters, RpcBlockSubscribe},
    config::{parse_market_filter, parse_pubkey_filter, MarketType},
    output::{parse_output_format, OutputFormat, WebhookConfig, WebhookNotifier},
    processors::{
        RaydiumAmmV4InstructionProcessor, RaydiumClmmInstructionProcessor,
        RaydiumCpmmInstructionProcessor,
    },
    solana_client::rpc_config::{RpcBlockSubscribeConfig, RpcBlockSubscribeFilter},
    solana_pubkey::Pubkey,
    std::{collections::HashSet, env, sync::Arc},
    tokio::signal,
};

#[tokio::main]
pub async fn main() -> CarbonResult<()> {
    dotenv::dotenv().ok();
    env_logger::init();

    // Create filter for block subscription
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
    let filter_markets = parse_market_filter("FILTER_MARKETS");
    let filter_tokens = parse_pubkey_filter("FILTER_TOKENS");
    let filter_amms = parse_pubkey_filter("FILTER_AMMS");
    let output_format = parse_output_format("OUTPUT_FORMAT");

    // Initialize optional webhook notifier
    let webhook_notifier = WebhookConfig::from_env().map(|config| {
        log::info!("Webhook notifications enabled: {}", config.url);
        Arc::new(WebhookNotifier::new(config))
    });

    log_startup_info(
        &rpc_ws_url,
        &filter_markets,
        &filter_tokens,
        &filter_amms,
        output_format,
        webhook_notifier.is_some(),
    );

    let block_subscribe = RpcBlockSubscribe::new(rpc_ws_url, filters);

    // Build pipeline with selected market processors
    let mut pipeline = build_pipeline(
        block_subscribe,
        &filter_markets,
        &filter_tokens,
        &filter_amms,
        output_format,
        webhook_notifier,
    )?;

    // Run pipeline with graceful shutdown on Ctrl+C
    tokio::select! {
        result = pipeline.run() => {
            result?;
        }
        _ = signal::ctrl_c() => {
            log::info!("Received Ctrl+C, shutting down...");
        }
    }

    Ok(())
}

/// Builds the pipeline with only the selected market processors.
///
/// This dynamically adds decoders based on `FILTER_MARKETS` configuration.
fn build_pipeline(
    datasource: RpcBlockSubscribe,
    filter_markets: &HashSet<MarketType>,
    filter_tokens: &HashSet<Pubkey>,
    filter_amms: &HashSet<Pubkey>,
    output_format: OutputFormat,
    webhook_notifier: Option<Arc<WebhookNotifier>>,
) -> CarbonResult<Pipeline> {
    let mut builder = Pipeline::builder()
        .datasource(datasource)
        .metrics(Arc::new(LogMetrics::new()))
        .metrics_flush_interval(3);

    // Add CPMM decoder if enabled
    if filter_markets.contains(&MarketType::Cpmm) {
        let processor = RaydiumCpmmInstructionProcessor::new(
            filter_tokens.clone(),
            filter_amms.clone(),
            output_format,
            webhook_notifier.clone(),
        );
        builder = builder.instruction(RaydiumCpmmDecoder, processor);
        log::info!("CPMM processor: enabled");
    } else {
        log::info!("CPMM processor: disabled");
    }

    // Add CLMM decoder if enabled
    if filter_markets.contains(&MarketType::Clmm) {
        let processor = RaydiumClmmInstructionProcessor::new(
            filter_tokens.clone(),
            filter_amms.clone(),
            output_format,
            webhook_notifier.clone(),
        );
        builder = builder.instruction(RaydiumClmmDecoder, processor);
        log::info!("CLMM processor: enabled");
    } else {
        log::info!("CLMM processor: disabled");
    }

    // Add AMM V4 decoder if enabled
    if filter_markets.contains(&MarketType::AmmV4) {
        let processor = RaydiumAmmV4InstructionProcessor::new(
            filter_amms.clone(),
            output_format,
            webhook_notifier,
        );
        builder = builder.instruction(RaydiumAmmV4Decoder, processor);
        log::info!("AMM V4 processor: enabled");
    } else {
        log::info!("AMM V4 processor: disabled");
    }

    builder
        .shutdown_strategy(carbon_core::pipeline::ShutdownStrategy::Immediate)
        .build()
}

/// Logs startup configuration information.
///
/// Displays program IDs and filter status for debugging and verification.
fn log_startup_info(
    rpc_ws_url: &str,
    filter_markets: &HashSet<MarketType>,
    filter_tokens: &HashSet<Pubkey>,
    filter_amms: &HashSet<Pubkey>,
    output_format: OutputFormat,
    webhook_enabled: bool,
) {
    log::info!("=== Raydium Alert System ===");
    log::info!("Raydium CPMM Program ID: {}", RAYDIUM_CPMM_PROGRAM_ID);
    log::info!("Raydium CLMM Program ID: {}", RAYDIUM_CLMM_PROGRAM_ID);
    log::info!("Raydium AMM V4 Program ID: {}", RAYDIUM_AMM_V4_PROGRAM_ID);

    // Log market filter status
    let market_names: Vec<&str> = filter_markets
        .iter()
        .map(|m| match m {
            MarketType::Cpmm => "cpmm",
            MarketType::Clmm => "clmm",
            MarketType::AmmV4 => "amm_v4",
        })
        .collect();
    log::info!("Markets filter: {:?}", market_names);

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

    // Log AMM/pool filter status
    if filter_amms.is_empty() {
        log::info!("AMM/Pool filter: disabled (tracking all AMMs/pools)");
    } else {
        log::info!(
            "AMM/Pool filter: {} address(es) - {:?}",
            filter_amms.len(),
            filter_amms
        );
    }

    // Log output settings
    log::info!(
        "Output format: {:?}",
        match output_format {
            OutputFormat::Text => "text",
            OutputFormat::Json => "json",
            OutputFormat::JsonPretty => "json_pretty",
        }
    );
    log::info!(
        "Webhook notifications: {}",
        if webhook_enabled {
            "enabled"
        } else {
            "disabled"
        }
    );

    log::info!("RPC WebSocket: {rpc_ws_url}");
    log::info!("============================");
}
