//! Normalized swap event data structure.
//!
//! This module provides a protocol-agnostic representation of swap events
//! that works across CPMM, CLMM, and AMM V4.

use {
    serde::{Deserialize, Serialize},
    solana_pubkey::Pubkey,
    std::{env, fmt, str::FromStr},
};

// Well-known token addresses for identification
/// Wrapped SOL mint address
pub const WSOL_MINT: &str = "So11111111111111111111111111111111111111112";
/// USDC mint address
pub const USDC_MINT: &str = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v";
/// USDT mint address
pub const USDT_MINT: &str = "Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB";

/// Raydium protocol type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Protocol {
    /// Constant Product Market Maker
    Cpmm,
    /// Concentrated Liquidity Market Maker
    Clmm,
    /// AMM V4 (legacy with Serum integration)
    AmmV4,
}

impl fmt::Display for Protocol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cpmm => write!(f, "CPMM"),
            Self::Clmm => write!(f, "CLMM"),
            Self::AmmV4 => write!(f, "AMM-V4"),
        }
    }
}

/// Swap direction indicator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SwapDirection {
    /// Swap specifies exact input amount, output is variable
    ExactInput,
    /// Swap specifies exact output amount, input is variable
    ExactOutput,
    /// Direction unknown (e.g., from event logs)
    #[default]
    Unknown,
}

impl fmt::Display for SwapDirection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ExactInput => write!(f, "exact_input"),
            Self::ExactOutput => write!(f, "exact_output"),
            Self::Unknown => write!(f, "unknown"),
        }
    }
}

/// Event type for different on-chain events.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum EventType {
    /// Token swap event
    #[default]
    Swap,
    /// Add liquidity event
    AddLiquidity,
    /// Remove liquidity event
    RemoveLiquidity,
    /// Pool creation event
    CreatePool,
}

impl fmt::Display for EventType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Swap => write!(f, "SWAP"),
            Self::AddLiquidity => write!(f, "ADD_LP"),
            Self::RemoveLiquidity => write!(f, "REMOVE_LP"),
            Self::CreatePool => write!(f, "CREATE_POOL"),
        }
    }
}

/// Token information with optional metadata.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TokenInfo {
    /// Token mint address
    pub mint: String,
    /// Token symbol (e.g., "SOL", "USDC") - if known
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbol: Option<String>,
    /// Token decimals (e.g., 9 for SOL, 6 for USDC)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decimals: Option<u8>,
    /// Raw amount in smallest units (lamports)
    pub amount_raw: u64,
    /// Human-readable amount (amount_raw / 10^decimals)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub amount: Option<f64>,
    /// USD value of the amount
    #[serde(skip_serializing_if = "Option::is_none")]
    pub amount_usd: Option<f64>,
}

impl TokenInfo {
    /// Creates a new TokenInfo with just mint and raw amount.
    pub fn new(mint: impl Into<String>, amount_raw: u64) -> Self {
        Self {
            mint: mint.into(),
            amount_raw,
            ..Default::default()
        }
    }

    /// Creates TokenInfo from a Pubkey.
    pub fn from_pubkey(mint: &Pubkey, amount_raw: u64) -> Self {
        Self::new(mint.to_string(), amount_raw)
    }

    /// Sets the token symbol.
    #[allow(dead_code)]
    pub fn with_symbol(mut self, symbol: impl Into<String>) -> Self {
        self.symbol = Some(symbol.into());
        self
    }

    /// Sets the decimals and calculates human-readable amount.
    #[allow(dead_code)]
    pub fn with_decimals(mut self, decimals: u8) -> Self {
        self.decimals = Some(decimals);
        self.amount = Some(self.amount_raw as f64 / 10_f64.powi(decimals as i32));
        self
    }

    /// Sets the USD value.
    #[allow(dead_code)]
    pub fn with_usd_value(mut self, usd: f64) -> Self {
        self.amount_usd = Some(usd);
        self
    }

    /// Formats the token for display.
    ///
    /// Returns format like: "🔷 SOL 11.9880 ($1491.19)" or "🪙 TOKEN 1234.56"
    pub fn format_display(&self, is_base: bool) -> String {
        let emoji = if is_base { "🔷" } else { "🪙" };
        let symbol = self.symbol.as_deref().unwrap_or(&self.mint[..8]);

        let amount_str = if let Some(amount) = self.amount {
            format!("{:.4}", amount)
        } else {
            format!("{}", self.amount_raw)
        };

        if let Some(usd) = self.amount_usd {
            format!("{} {} {} (${:.2})", emoji, symbol, amount_str, usd)
        } else {
            format!("{} {} {}", emoji, symbol, amount_str)
        }
    }

    /// Checks if this token is a well-known base token (SOL, USDC, USDT).
    pub fn is_base_token(&self) -> bool {
        matches!(self.mint.as_str(), WSOL_MINT | USDC_MINT | USDT_MINT)
    }
}

/// Normalized swap event that abstracts protocol differences.
///
/// This structure provides a unified view of swap events across CPMM, CLMM, and AMM V4,
/// making it easy to process, log, and alert on swaps regardless of the underlying protocol.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwapEvent {
    /// Event type (Swap, AddLiquidity, RemoveLiquidity, etc.)
    pub event_type: EventType,

    /// Protocol that emitted this event
    pub protocol: Protocol,

    /// Transaction signature
    pub signature: String,

    /// Pool or AMM address
    pub pool: String,

    /// Input token information
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_token: Option<TokenInfo>,

    /// Output token information
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_token: Option<TokenInfo>,

    /// Swap direction (exact input, exact output, or unknown)
    pub direction: SwapDirection,

    /// Trading fee in raw token units (if available)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fee: Option<u64>,

    /// Maker/sender address
    #[serde(skip_serializing_if = "Option::is_none")]
    pub maker: Option<String>,

    /// Market cap of the non-base token (if available)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub market_cap_usd: Option<f64>,

    /// Block slot number
    pub slot: u64,

    /// Unix timestamp (seconds since epoch, if available)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<i64>,
}

impl SwapEvent {
    /// Creates a new swap event builder.
    pub fn builder() -> SwapEventBuilder {
        SwapEventBuilder::default()
    }

    /// Formats the swap event according to the specified output format.
    pub fn format(&self, format: OutputFormat) -> String {
        match format {
            OutputFormat::Text => self.format_text(),
            OutputFormat::Json => self.format_json(),
            OutputFormat::JsonPretty => self.format_json_pretty(),
        }
    }

    /// Formats as emoji-rich human-readable text.
    ///
    /// Example output:
    /// ```text
    /// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
    /// 🔄 SWAP [CPMM]
    /// 🔷 SOL 11.9880 ($1491.19)
    /// 🪙 MACARON 11500.70
    /// 🔎 Maker: 7xKXt...
    /// 📈 MCap: $615,340
    /// 🔗 https://solscan.io/tx/...
    /// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
    /// ```
    fn format_text(&self) -> String {
        let mut lines = Vec::new();

        // Header with event type and protocol
        let event_emoji = match self.event_type {
            EventType::Swap => "🔄",
            EventType::AddLiquidity => "💧",
            EventType::RemoveLiquidity => "🔥",
            EventType::CreatePool => "🆕",
        };
        lines.push(format!(
            "{} {} [{}]",
            event_emoji, self.event_type, self.protocol
        ));

        // Determine which token is base and which is quote
        let (base_token, quote_token) = self.get_base_quote_tokens();

        // Token amounts - base token first (usually SOL/USDC)
        if let Some(token) = base_token {
            lines.push(token.format_display(true));
        }
        if let Some(token) = quote_token {
            lines.push(token.format_display(false));
        }

        // Maker address (shortened)
        if let Some(ref maker) = self.maker {
            let short_maker = if maker.len() > 12 {
                format!("{}...{}", &maker[..6], &maker[maker.len() - 4..])
            } else {
                maker.clone()
            };
            lines.push(format!("🔎 Maker: {}", short_maker));
        }

        // Market cap
        if let Some(mcap) = self.market_cap_usd {
            lines.push(format!("📈 MCap: ${}", format_number(mcap)));
        }

        // Fee if available
        if let Some(fee) = self.fee {
            lines.push(format!("💰 Fee: {}", fee));
        }

        // Transaction link
        let short_sig = if self.signature.len() > 12 {
            format!("{}...", &self.signature[..12])
        } else {
            self.signature.clone()
        };
        lines.push(format!("🔗 https://solscan.io/tx/{}", short_sig));

        lines.join("\n")
    }

    /// Gets the base and quote tokens, ordering so base tokens (SOL/USDC) come first.
    fn get_base_quote_tokens(&self) -> (Option<&TokenInfo>, Option<&TokenInfo>) {
        match (&self.input_token, &self.output_token) {
            (Some(input), Some(output)) => {
                // If output is base token (selling quote for base), swap order for display
                if output.is_base_token() && !input.is_base_token() {
                    (Some(output), Some(input))
                } else {
                    (Some(input), Some(output))
                }
            }
            (Some(input), None) => (Some(input), None),
            (None, Some(output)) => (Some(output), None),
            (None, None) => (None, None),
        }
    }

    /// Formats as compact JSON.
    fn format_json(&self) -> String {
        serde_json::to_string(self)
            .unwrap_or_else(|e| format!("{{\"error\": \"serialization failed: {e}\"}}"))
    }

    /// Formats as pretty-printed JSON.
    fn format_json_pretty(&self) -> String {
        serde_json::to_string_pretty(self)
            .unwrap_or_else(|e| format!("{{\n  \"error\": \"serialization failed: {e}\"\n}}"))
    }

    /// Calculates the effective price (output per input).
    ///
    /// Returns `None` if amounts are not available or input is zero.
    #[allow(dead_code)]
    pub fn price(&self) -> Option<f64> {
        let input = self.input_token.as_ref()?.amount?;
        let output = self.output_token.as_ref()?.amount?;
        if input == 0.0 {
            return None;
        }
        Some(output / input)
    }

    /// Calculates the inverse price (input per output).
    ///
    /// Returns `None` if amounts are not available or output is zero.
    #[allow(dead_code)]
    pub fn inverse_price(&self) -> Option<f64> {
        let input = self.input_token.as_ref()?.amount?;
        let output = self.output_token.as_ref()?.amount?;
        if output == 0.0 {
            return None;
        }
        Some(input / output)
    }

    /// Gets the total USD value of the swap (input or output, whichever is available).
    #[allow(dead_code)]
    pub fn usd_value(&self) -> Option<f64> {
        self.input_token
            .as_ref()
            .and_then(|t| t.amount_usd)
            .or_else(|| self.output_token.as_ref().and_then(|t| t.amount_usd))
    }
}

/// Formats a number with thousands separators.
fn format_number(n: f64) -> String {
    if n >= 1_000_000_000.0 {
        format!("{:.2}B", n / 1_000_000_000.0)
    } else if n >= 1_000_000.0 {
        format!("{:.2}M", n / 1_000_000.0)
    } else if n >= 1_000.0 {
        format!("{:.2}K", n / 1_000.0)
    } else {
        format!("{:.2}", n)
    }
}

/// Builder for constructing SwapEvent instances.
#[derive(Debug, Default)]
pub struct SwapEventBuilder {
    event_type: EventType,
    protocol: Option<Protocol>,
    signature: Option<String>,
    pool: Option<String>,
    input_token: Option<TokenInfo>,
    output_token: Option<TokenInfo>,
    direction: SwapDirection,
    fee: Option<u64>,
    maker: Option<String>,
    market_cap_usd: Option<f64>,
    slot: u64,
    timestamp: Option<i64>,
}

impl SwapEventBuilder {
    /// Sets the event type.
    pub fn event_type(mut self, event_type: EventType) -> Self {
        self.event_type = event_type;
        self
    }

    /// Sets the protocol.
    pub fn protocol(mut self, protocol: Protocol) -> Self {
        self.protocol = Some(protocol);
        self
    }

    /// Sets the transaction signature.
    pub fn signature(mut self, sig: impl Into<String>) -> Self {
        self.signature = Some(sig.into());
        self
    }

    /// Sets the pool/AMM address.
    pub fn pool(mut self, pool: impl Into<String>) -> Self {
        self.pool = Some(pool.into());
        self
    }

    /// Sets the pool/AMM address from a Pubkey.
    pub fn pool_pubkey(mut self, pool: &Pubkey) -> Self {
        self.pool = Some(pool.to_string());
        self
    }

    /// Sets the input token information.
    pub fn input_token(mut self, token: TokenInfo) -> Self {
        self.input_token = Some(token);
        self
    }

    /// Sets the input token from mint and amount.
    #[allow(dead_code)]
    pub fn input_mint_amount(mut self, mint: &Pubkey, amount: u64) -> Self {
        self.input_token = Some(TokenInfo::from_pubkey(mint, amount));
        self
    }

    /// Sets the output token information.
    pub fn output_token(mut self, token: TokenInfo) -> Self {
        self.output_token = Some(token);
        self
    }

    /// Sets the output token from mint and amount.
    #[allow(dead_code)]
    pub fn output_mint_amount(mut self, mint: &Pubkey, amount: u64) -> Self {
        self.output_token = Some(TokenInfo::from_pubkey(mint, amount));
        self
    }

    /// Sets the swap direction.
    pub fn direction(mut self, direction: SwapDirection) -> Self {
        self.direction = direction;
        self
    }

    /// Sets the trading fee.
    pub fn fee(mut self, fee: u64) -> Self {
        self.fee = Some(fee);
        self
    }

    /// Sets the maker/sender address.
    #[allow(dead_code)]
    pub fn maker(mut self, maker: impl Into<String>) -> Self {
        self.maker = Some(maker.into());
        self
    }

    /// Sets the maker from a Pubkey.
    pub fn maker_pubkey(mut self, maker: &Pubkey) -> Self {
        self.maker = Some(maker.to_string());
        self
    }

    /// Sets the market cap in USD.
    #[allow(dead_code)]
    pub fn market_cap_usd(mut self, mcap: f64) -> Self {
        self.market_cap_usd = Some(mcap);
        self
    }

    /// Sets the block slot.
    pub fn slot(mut self, slot: u64) -> Self {
        self.slot = slot;
        self
    }

    /// Sets the timestamp.
    #[allow(dead_code)]
    pub fn timestamp(mut self, ts: i64) -> Self {
        self.timestamp = Some(ts);
        self
    }

    /// Builds the SwapEvent.
    ///
    /// # Panics
    ///
    /// Panics if `protocol`, `signature`, or `pool` are not set.
    pub fn build(self) -> SwapEvent {
        SwapEvent {
            event_type: self.event_type,
            protocol: self.protocol.expect("protocol is required"),
            signature: self.signature.expect("signature is required"),
            pool: self.pool.expect("pool is required"),
            input_token: self.input_token,
            output_token: self.output_token,
            direction: self.direction,
            fee: self.fee,
            maker: self.maker,
            market_cap_usd: self.market_cap_usd,
            slot: self.slot,
            timestamp: self.timestamp,
        }
    }
}

/// Output format for swap events.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OutputFormat {
    /// Human-readable text format with emojis (default)
    #[default]
    Text,
    /// Compact JSON format (one line per event)
    Json,
    /// Pretty-printed JSON format
    JsonPretty,
}

impl FromStr for OutputFormat {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().trim() {
            "text" | "txt" => Ok(Self::Text),
            "json" => Ok(Self::Json),
            "json_pretty" | "json-pretty" | "jsonpretty" => Ok(Self::JsonPretty),
            _ => Err(format!(
                "Unknown output format: '{s}'. Valid options: text, json, json_pretty"
            )),
        }
    }
}

/// Parses the output format from an environment variable.
///
/// # Arguments
///
/// * `env_var` - The name of the environment variable to read
///
/// # Returns
///
/// Returns the parsed `OutputFormat`, defaulting to `Text` if not set or invalid.
pub fn parse_output_format(env_var: &str) -> OutputFormat {
    env::var(env_var)
        .ok()
        .and_then(|val| {
            let trimmed = val.trim();
            if trimmed.is_empty() {
                return None;
            }
            match OutputFormat::from_str(trimmed) {
                Ok(f) => Some(f),
                Err(e) => {
                    log::warn!("{}", e);
                    None
                }
            }
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_token_info_display() {
        let token = TokenInfo::new(
            "So11111111111111111111111111111111111111112",
            11_988_000_000,
        )
        .with_symbol("SOL")
        .with_decimals(9)
        .with_usd_value(1491.19);

        let display = token.format_display(true);
        assert!(display.contains("🔷"));
        assert!(display.contains("SOL"));
        assert!(display.contains("11.9880"));
        assert!(display.contains("$1491.19"));
    }

    #[test]
    fn test_token_info_without_usd() {
        let token = TokenInfo::new("TokenMint123", 11500_700_000)
            .with_symbol("MACARON")
            .with_decimals(6);

        let display = token.format_display(false);
        assert!(display.contains("🪙"));
        assert!(display.contains("MACARON"));
        assert!(display.contains("11500.7000"));
        assert!(!display.contains("$"));
    }

    #[test]
    fn test_swap_event_text_format() {
        let input_token = TokenInfo::new(WSOL_MINT, 11_988_000_000)
            .with_symbol("SOL")
            .with_decimals(9)
            .with_usd_value(1491.19);

        let output_token = TokenInfo::new("MacaronMint123", 11_500_700_000)
            .with_symbol("MACARON")
            .with_decimals(6);

        let event = SwapEvent::builder()
            .protocol(Protocol::Cpmm)
            .signature("5abc123def456")
            .pool("pool123")
            .input_token(input_token)
            .output_token(output_token)
            .maker("7xKXtQRzdP9WmUHQzNJJfJnRhPs8")
            .market_cap_usd(615340.0)
            .slot(12345)
            .build();

        let text = event.format(OutputFormat::Text);
        assert!(text.contains("🔄 SWAP"));
        assert!(text.contains("[CPMM]"));
        assert!(text.contains("SOL"));
        assert!(text.contains("MACARON"));
        assert!(text.contains("Maker:"));
        assert!(text.contains("MCap: $615.34K"));
        assert!(text.contains("solscan.io"));
    }

    #[test]
    fn test_swap_event_json_format() {
        let event = SwapEvent::builder()
            .protocol(Protocol::Clmm)
            .signature("sig123")
            .pool("pool456")
            .input_token(TokenInfo::new("mint_in", 100))
            .output_token(TokenInfo::new("mint_out", 200))
            .slot(999)
            .build();

        let json = event.format(OutputFormat::Json);
        assert!(json.contains("\"protocol\":\"clmm\""));
        assert!(json.contains("\"signature\":\"sig123\""));
    }

    #[test]
    fn test_format_number() {
        assert_eq!(format_number(500.0), "500.00");
        assert_eq!(format_number(1500.0), "1.50K");
        assert_eq!(format_number(615340.0), "615.34K");
        assert_eq!(format_number(1_500_000.0), "1.50M");
        assert_eq!(format_number(2_500_000_000.0), "2.50B");
    }

    #[test]
    fn test_is_base_token() {
        let sol = TokenInfo::new(WSOL_MINT, 0);
        let usdc = TokenInfo::new(USDC_MINT, 0);
        let random = TokenInfo::new("RandomMint123", 0);

        assert!(sol.is_base_token());
        assert!(usdc.is_base_token());
        assert!(!random.is_base_token());
    }

    #[test]
    fn test_output_format_from_str() {
        assert_eq!(OutputFormat::from_str("text").unwrap(), OutputFormat::Text);
        assert_eq!(OutputFormat::from_str("json").unwrap(), OutputFormat::Json);
        assert_eq!(
            OutputFormat::from_str("json_pretty").unwrap(),
            OutputFormat::JsonPretty
        );
        assert!(OutputFormat::from_str("invalid").is_err());
    }
}
