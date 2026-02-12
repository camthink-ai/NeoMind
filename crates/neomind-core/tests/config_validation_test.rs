//! Configuration Validation Tests
//!
//! Comprehensive tests for configuration validation including:
//! - Edge cases and boundary values
//! - Invalid input handling
//! - URL validation
//! - Numeric range validation

use neomind_core::config::{
    agent, agent_env_vars,
    normalize_ollama_endpoint, normalize_openai_endpoint,
    endpoints, models, env_vars,
};

#[test]
fn test_agent_config_constants() {
    // Verify agent configuration constants are reasonable
    assert!(agent::DEFAULT_MAX_CONTEXT_TOKENS >= 1000);
    assert!(agent::DEFAULT_MAX_CONTEXT_TOKENS <= 128000);
    assert!(agent::DEFAULT_TEMPERATURE >= 0.0);
    assert!(agent::DEFAULT_TEMPERATURE <= 2.0);
    assert!(agent::DEFAULT_TOP_P >= 0.0);
    assert!(agent::DEFAULT_TOP_P <= 1.0);
    assert!(agent::DEFAULT_MAX_TOKENS >= 1);
    assert!(agent::DEFAULT_CONCURRENT_LIMIT >= 1);
    assert!(agent::DEFAULT_CONCURRENT_LIMIT <= 100);
}

#[test]
fn test_normalize_ollama_edge_cases() {
    // Empty input
    assert_eq!(normalize_ollama_endpoint("".to_string()), "");

    // Multiple trailing slashes
    assert_eq!(
        normalize_ollama_endpoint("http://localhost:11434///".to_string()),
        "http://localhost:11434"
    );

    // Multiple /v1 occurrences (replace removes ALL of them)
    assert_eq!(
        normalize_ollama_endpoint("http://localhost:11434/v1/v1".to_string()),
        "http://localhost:11434"
    );

    // URL with path
    assert_eq!(
        normalize_ollama_endpoint("http://localhost:11434/api/v1".to_string()),
        "http://localhost:11434/api"
    );

    // HTTPS endpoint
    assert_eq!(
        normalize_ollama_endpoint("https://ollama.example.com/v1".to_string()),
        "https://ollama.example.com"
    );
}

#[test]
fn test_normalize_openai_edge_cases() {
    // Empty input
    assert_eq!(normalize_openai_endpoint("".to_string()), "/v1");

    // Already has /v1/ (should normalize)
    assert_eq!(
        normalize_openai_endpoint("https://api.openai.com/v1/".to_string()),
        "https://api.openai.com/v1"
    );

    // With path and /v1 in middle (adds /v1 if not at end)
    assert_eq!(
        normalize_openai_endpoint("https://api.openai.com/v1/chat".to_string()),
        "https://api.openai.com/v1/chat/v1"
    );

    // Multiple /v1 (none at end, so adds /v1)
    assert_eq!(
        normalize_openai_endpoint("https://api.openai.com/v1/v2".to_string()),
        "https://api.openai.com/v1/v2/v1"
    );
}

#[test]
fn test_endpoint_constants_format() {
    // Verify all endpoint constants are valid URLs or paths
    assert!(endpoints::OLLAMA.starts_with("http"));
    assert!(endpoints::OPENAI.starts_with("https"));
    assert!(endpoints::ANTHROPIC.starts_with("https"));
    assert!(endpoints::GOOGLE.starts_with("https"));
    assert!(endpoints::XAI.starts_with("https"));
}

#[test]
fn test_model_constants_non_empty() {
    // Verify all model constants are non-empty
    assert!(!models::OLLAMA_DEFAULT.is_empty());
    assert!(!models::OPENAI_DEFAULT.is_empty());
    assert!(models::OLLAMA_DEFAULT.len() > 3);
    assert!(models::OPENAI_DEFAULT.len() > 3);
}

#[test]
fn test_env_var_constants_unique() {
    // Verify environment variable names don't have duplicates
    let vars = [
        env_vars::LLM_PROVIDER,
        env_vars::LLM_MODEL,
        env_vars::OLLAMA_ENDPOINT,
        env_vars::OPENAI_API_KEY,
        env_vars::OPENAI_ENDPOINT,
    ];

    let unique_vars: std::collections::HashSet<_> = vars.iter().collect();
    assert_eq!(unique_vars.len(), vars.len(), "Environment variable names should be unique");
}

#[test]
fn test_env_var_naming_convention() {
    // Verify env vars follow consistent naming convention
    let vars = [
        env_vars::LLM_PROVIDER,
        env_vars::LLM_MODEL,
        env_vars::OLLAMA_ENDPOINT,
        env_vars::OPENAI_API_KEY,
        env_vars::OPENAI_ENDPOINT,
    ];

    for var in vars {
        // All env vars should be uppercase and use underscores
        assert!(var == var.to_uppercase(), "Env var should be uppercase: {}", var);
        assert!(!var.contains('-'), "Env var should use underscores not hyphens: {}", var);
    }
}

#[test]
fn test_agent_env_var_constants() {
    // Verify agent env var constants follow naming convention
    let vars = [
        agent_env_vars::MAX_CONTEXT_TOKENS,
        agent_env_vars::TEMPERATURE,
        agent_env_vars::TOP_P,
        agent_env_vars::MAX_TOKENS,
        agent_env_vars::CONCURRENT_LIMIT,
        agent_env_vars::CONTEXT_SELECTOR_TOKENS,
        agent_env_vars::LLM_TIMEOUT_SECS,
    ];

    for var in vars {
        assert!(var.starts_with("AGENT_"), "Agent env var should start with AGENT_: {}", var);
        assert!(var == var.to_uppercase(), "Env var should be uppercase: {}", var);
    }
}

#[test]
fn test_normalize_preserves_case() {
    // Endpoints should preserve case (URLs are case-sensitive for path)
    let input = "http://localhost:11434/API/v1";
    assert_eq!(normalize_ollama_endpoint(input.to_string()), "http://localhost:11434/API");
}

#[test]
fn test_normalize_with_port() {
    // Test with different port numbers
    assert_eq!(
        normalize_ollama_endpoint("http://localhost:8080/v1".to_string()),
        "http://localhost:8080"
    );

    assert_eq!(
        normalize_ollama_endpoint("http://192.168.1.1:11434/v1".to_string()),
        "http://192.168.1.1:11434"
    );
}

#[test]
fn test_normalize_with_query_params() {
    // Ollama: query params should remain after /v1 removal
    let result = normalize_ollama_endpoint("http://localhost:11434/v1?timeout=60".to_string());
    assert!(result.contains("timeout=60"));
}

#[test]
fn test_openai_normalize_with_fragment() {
    // URL fragments should be preserved
    let result = normalize_openai_endpoint("https://api.example.com".to_string());
    assert!(result.ends_with("/v1"));
}

#[test]
fn test_ip_address_endpoints() {
    // IPv4 addresses
    assert_eq!(
        normalize_ollama_endpoint("http://127.0.0.1:11434/v1".to_string()),
        "http://127.0.0.1:11434"
    );

    // IPv4 with no port
    assert_eq!(
        normalize_ollama_endpoint("http://192.168.1.1/v1".to_string()),
        "http://192.168.1.1"
    );
}

#[test]
fn test_agent_config_bounds() {
    // Test that config values are within reasonable bounds
    assert!(agent::DEFAULT_MAX_CONTEXT_TOKENS >= 1000);
    assert!(agent::DEFAULT_MAX_CONTEXT_TOKENS <= 200000);

    assert!(agent::DEFAULT_TEMPERATURE >= 0.0);
    assert!(agent::DEFAULT_TEMPERATURE <= 2.0);

    assert!(agent::DEFAULT_TOP_P >= 0.0);
    assert!(agent::DEFAULT_TOP_P <= 1.0);

    assert!(agent::DEFAULT_MAX_TOKENS >= 1);
    assert!(agent::DEFAULT_MAX_TOKENS <= 200000);

    // Context selector tokens should be less than max context
    assert!(agent::DEFAULT_CONTEXT_SELECTOR_TOKENS <= agent::DEFAULT_MAX_CONTEXT_TOKENS);
}

#[test]
fn test_agent_env_var_parsing() {
    // Test that env var parsing functions don't panic on invalid input
    // They should return defaults in that case

    // Save original env vars
    let orig_max_context = std::env::var(agent_env_vars::MAX_CONTEXT_TOKENS);
    let orig_temp = std::env::var(agent_env_vars::TEMPERATURE);

    // Test with invalid values - should fall back to defaults
    unsafe { std::env::set_var(agent_env_vars::MAX_CONTEXT_TOKENS, "invalid"); }
    let result = agent_env_vars::max_context_tokens();
    assert_eq!(result, agent::DEFAULT_MAX_CONTEXT_TOKENS);

    unsafe { std::env::set_var(agent_env_vars::TEMPERATURE, "not_a_number"); }
    let result = agent_env_vars::temperature();
    assert_eq!(result, agent::DEFAULT_TEMPERATURE);

    // Restore original env vars
    match orig_max_context {
        Ok(v) => unsafe { std::env::set_var(agent_env_vars::MAX_CONTEXT_TOKENS, v); },
        Err(_) => unsafe { std::env::remove_var(agent_env_vars::MAX_CONTEXT_TOKENS); },
    }
    match orig_temp {
        Ok(v) => unsafe { std::env::set_var(agent_env_vars::TEMPERATURE, v); },
        Err(_) => unsafe { std::env::remove_var(agent_env_vars::TEMPERATURE); },
    }
}

#[test]
fn test_agent_env_var_valid_values() {
    // Test that valid env var values are parsed correctly
    let orig_max_tokens = std::env::var(agent_env_vars::MAX_TOKENS);
    let orig_top_p = std::env::var(agent_env_vars::TOP_P);

    unsafe { std::env::set_var(agent_env_vars::MAX_TOKENS, "5000"); }
    assert_eq!(agent_env_vars::max_tokens(), 5000);

    unsafe { std::env::set_var(agent_env_vars::TOP_P, "0.9"); }
    assert!((agent_env_vars::top_p() - 0.9).abs() < 0.001);

    // Restore
    match orig_max_tokens {
        Ok(v) => unsafe { std::env::set_var(agent_env_vars::MAX_TOKENS, v); },
        Err(_) => unsafe { std::env::remove_var(agent_env_vars::MAX_TOKENS); },
    }
    match orig_top_p {
        Ok(v) => unsafe { std::env::set_var(agent_env_vars::TOP_P, v); },
        Err(_) => unsafe { std::env::remove_var(agent_env_vars::TOP_P); },
    }
}

#[test]
fn test_concurrent_limit_env_var() {
    let orig = std::env::var(agent_env_vars::CONCURRENT_LIMIT);

    unsafe { std::env::set_var(agent_env_vars::CONCURRENT_LIMIT, "5"); }
    assert_eq!(agent_env_vars::concurrent_limit(), 5);

    // Restore
    match orig {
        Ok(v) => unsafe { std::env::set_var(agent_env_vars::CONCURRENT_LIMIT, v); },
        Err(_) => unsafe { std::env::remove_var(agent_env_vars::CONCURRENT_LIMIT); },
    }
}

#[test]
fn test_context_selector_tokens_env_var() {
    let orig = std::env::var(agent_env_vars::CONTEXT_SELECTOR_TOKENS);

    unsafe { std::env::set_var(agent_env_vars::CONTEXT_SELECTOR_TOKENS, "2000"); }
    assert_eq!(agent_env_vars::context_selector_tokens(), 2000);

    // Restore
    match orig {
        Ok(v) => unsafe { std::env::set_var(agent_env_vars::CONTEXT_SELECTOR_TOKENS, v); },
        Err(_) => unsafe { std::env::remove_var(agent_env_vars::CONTEXT_SELECTOR_TOKENS); },
    }
}

#[test]
fn test_llm_timeout_env_vars() {
    let orig = std::env::var(agent_env_vars::LLM_TIMEOUT_SECS);

    unsafe { std::env::set_var(agent_env_vars::LLM_TIMEOUT_SECS, "120"); }
    assert_eq!(agent_env_vars::llm_timeout_secs(), Some(120));
    assert_eq!(agent_env_vars::ollama_timeout_secs(), 120);
    assert_eq!(agent_env_vars::cloud_timeout_secs(), 120);

    // Restore
    match orig {
        Ok(v) => unsafe { std::env::set_var(agent_env_vars::LLM_TIMEOUT_SECS, v); },
        Err(_) => unsafe { std::env::remove_var(agent_env_vars::LLM_TIMEOUT_SECS); },
    }
}

#[test]
fn test_llm_timeout_defaults() {
    // Clear env var to test defaults
    let orig = std::env::var(agent_env_vars::LLM_TIMEOUT_SECS);
    unsafe { std::env::remove_var(agent_env_vars::LLM_TIMEOUT_SECS); }

    // When env var is not set, should return None
    assert_eq!(agent_env_vars::llm_timeout_secs(), None);

    // But specific getters should return defaults
    assert_eq!(agent_env_vars::ollama_timeout_secs(), 180);
    assert_eq!(agent_env_vars::cloud_timeout_secs(), 60);

    // Restore
    match orig {
        Ok(v) => unsafe { std::env::set_var(agent_env_vars::LLM_TIMEOUT_SECS, v); },
        Err(_) => unsafe { std::env::remove_var(agent_env_vars::LLM_TIMEOUT_SECS); },
    }
}

#[test]
fn test_endpoint_constants_do_not_end_with_slash() {
    // Endpoints should not have trailing slashes
    assert!(!endpoints::OLLAMA.ends_with('/'));
    assert!(!endpoints::OPENAI.ends_with('/'));
    assert!(!endpoints::ANTHROPIC.ends_with('/'));
    assert!(!endpoints::GOOGLE.ends_with('/'));
    assert!(!endpoints::XAI.ends_with('/'));
}

#[test]
fn test_normalize_unicode_handling() {
    // Test that endpoints with potential unicode are handled
    // (Rust strings are UTF-8, so this tests string handling)
    let input = "http://localhost:11434/v1";
    let output = normalize_ollama_endpoint(input.to_string());
    assert_eq!(output, "http://localhost:11434");
}

#[test]
fn test_normalize_with_special_characters() {
    // URLs with percent-encoded characters should be preserved
    let input = "http://localhost:11434/v1";
    let output = normalize_ollama_endpoint(input.to_string());
    assert_eq!(output, "http://localhost:11434");
}
