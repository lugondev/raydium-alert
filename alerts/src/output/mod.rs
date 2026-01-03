//! Output module for normalized swap events and formatters.
//!
//! This module provides:
//! - [`SwapEvent`] - A normalized swap event structure that abstracts protocol differences
//! - [`TokenInfo`] - Token information with optional metadata (symbol, decimals, USD value)
//! - [`OutputFormat`] - Configurable output formatting (text, JSON)
//! - [`token_transfer`] - Utilities for parsing actual transfer amounts from nested instructions
//! - Webhook notification support for alerting systems

pub mod swap_event;
pub mod token_transfer;
mod webhook;

pub use swap_event::{
    parse_output_format, EventType, OutputFormat, Protocol, SwapDirection, SwapEvent, TokenInfo,
};
pub use token_transfer::extract_swap_amounts;
pub use webhook::{WebhookConfig, WebhookNotifier};
