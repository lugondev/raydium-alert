//! Configuration module for parsing environment variables and filter settings.
//!
//! This module provides utilities for loading pubkey-based filters from environment
//! variables, commonly used for filtering by token mints or AMM pool addresses.

use {solana_pubkey::Pubkey, std::collections::HashSet, std::env, std::str::FromStr};

/// Supported Raydium market types for filtering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MarketType {
    /// Constant Product Market Maker
    Cpmm,
    /// Concentrated Liquidity Market Maker
    Clmm,
    /// AMM V4 (legacy with Serum integration)
    AmmV4,
}

impl FromStr for MarketType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // Case-insensitive matching, support common variations
        match s.to_lowercase().trim() {
            "cpmm" => Ok(Self::Cpmm),
            "clmm" => Ok(Self::Clmm),
            "amm_v4" | "ammv4" | "amm-v4" | "v4" => Ok(Self::AmmV4),
            _ => Err(format!(
                "Unknown market type: '{s}'. Valid options: cpmm, clmm, amm_v4"
            )),
        }
    }
}

/// Parses a comma-separated list of market types from an environment variable.
///
/// # Arguments
///
/// * `env_var` - The name of the environment variable to read
///
/// # Returns
///
/// A `HashSet` of `MarketType`. Returns a set with all market types if the env var
/// is not set or empty (default behavior: listen to all markets).
///
/// # Examples
///
/// ```ignore
/// // Set FILTER_MARKETS=clmm,cpmm to only listen to CLMM and CPMM
/// let markets = parse_market_filter("FILTER_MARKETS");
/// assert!(markets.contains(&MarketType::Clmm));
/// assert!(markets.contains(&MarketType::Cpmm));
/// assert!(!markets.contains(&MarketType::AmmV4));
///
/// // Empty or unset = all markets
/// let all_markets = parse_market_filter("UNSET_VAR");
/// assert_eq!(all_markets.len(), 3); // cpmm, clmm, amm_v4
/// ```
pub fn parse_market_filter(env_var: &str) -> HashSet<MarketType> {
    env::var(env_var)
        .ok()
        .and_then(|val| {
            let trimmed = val.trim();
            if trimmed.is_empty() {
                return None;
            }

            let markets: HashSet<MarketType> = trimmed
                .split(',')
                .filter_map(|s| {
                    let market_str = s.trim();
                    if market_str.is_empty() {
                        return None;
                    }
                    match MarketType::from_str(market_str) {
                        Ok(m) => Some(m),
                        Err(e) => {
                            log::warn!("{}", e);
                            None
                        }
                    }
                })
                .collect();

            // If parsing resulted in empty set (all invalid), return None to use default
            if markets.is_empty() {
                None
            } else {
                Some(markets)
            }
        })
        // Default: all market types enabled
        .unwrap_or_else(|| {
            let mut all = HashSet::new();
            all.insert(MarketType::Cpmm);
            all.insert(MarketType::Clmm);
            all.insert(MarketType::AmmV4);
            all
        })
}

/// Parses a comma-separated list of pubkey addresses from an environment variable.
///
/// # Arguments
///
/// * `env_var` - The name of the environment variable to read
///
/// # Returns
///
/// A `HashSet` of `Pubkey` addresses. Returns empty set if the env var is not set or empty.
///
/// # Examples
///
/// ```ignore
/// // Set FILTER_TOKENS=So11111111111111111111111111111111111111112,EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v
/// let tokens = parse_pubkey_filter("FILTER_TOKENS");
/// assert_eq!(tokens.len(), 2);
/// ```
pub fn parse_pubkey_filter(env_var: &str) -> HashSet<Pubkey> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_pubkey_filter_empty() {
        // Non-existent env var should return empty set
        let result = parse_pubkey_filter("NON_EXISTENT_VAR_12345");
        assert!(result.is_empty());
    }

    #[test]
    fn test_parse_pubkey_filter_with_whitespace() {
        env::set_var(
            "TEST_PUBKEYS",
            "  So11111111111111111111111111111111111111112  , EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v  ",
        );
        let result = parse_pubkey_filter("TEST_PUBKEYS");
        assert_eq!(result.len(), 2);
        env::remove_var("TEST_PUBKEYS");
    }

    #[test]
    fn test_market_type_from_str() {
        assert_eq!(MarketType::from_str("cpmm").unwrap(), MarketType::Cpmm);
        assert_eq!(MarketType::from_str("CPMM").unwrap(), MarketType::Cpmm);
        assert_eq!(MarketType::from_str("clmm").unwrap(), MarketType::Clmm);
        assert_eq!(MarketType::from_str("amm_v4").unwrap(), MarketType::AmmV4);
        assert_eq!(MarketType::from_str("ammv4").unwrap(), MarketType::AmmV4);
        assert_eq!(MarketType::from_str("v4").unwrap(), MarketType::AmmV4);
        assert!(MarketType::from_str("invalid").is_err());
    }

    #[test]
    fn test_parse_market_filter_default() {
        // Non-existent env var should return all markets
        let result = parse_market_filter("NON_EXISTENT_MARKET_VAR_12345");
        assert_eq!(result.len(), 3);
        assert!(result.contains(&MarketType::Cpmm));
        assert!(result.contains(&MarketType::Clmm));
        assert!(result.contains(&MarketType::AmmV4));
    }

    #[test]
    fn test_parse_market_filter_specific() {
        env::set_var("TEST_MARKETS", "clmm, cpmm");
        let result = parse_market_filter("TEST_MARKETS");
        assert_eq!(result.len(), 2);
        assert!(result.contains(&MarketType::Clmm));
        assert!(result.contains(&MarketType::Cpmm));
        assert!(!result.contains(&MarketType::AmmV4));
        env::remove_var("TEST_MARKETS");
    }

    #[test]
    fn test_parse_market_filter_single() {
        env::set_var("TEST_SINGLE_MARKET", "amm_v4");
        let result = parse_market_filter("TEST_SINGLE_MARKET");
        assert_eq!(result.len(), 1);
        assert!(result.contains(&MarketType::AmmV4));
        env::remove_var("TEST_SINGLE_MARKET");
    }

    #[test]
    fn test_parse_market_filter_empty_string() {
        env::set_var("TEST_EMPTY_MARKET", "");
        let result = parse_market_filter("TEST_EMPTY_MARKET");
        // Empty string should return all markets (default)
        assert_eq!(result.len(), 3);
        env::remove_var("TEST_EMPTY_MARKET");
    }
}
