pub mod client_graph_account;
pub mod client_graph_node;
pub mod client_network;
pub mod client_registry;

#[derive(Debug, thiserror::Error)]
pub enum QueryError {
    #[error(transparent)]
    Transport(#[from] reqwest::Error),
    #[error("The subgraph is in a failed state")]
    IndexingError,
    #[error("Query response is unexpected: {0}")]
    ParseResponseError(String),
    #[error("Query response is empty: {0}")]
    PrometheusError(#[from] prometheus_http_query::Error),
    #[error("Unknown error: {0}")]
    Other(anyhow::Error),
}

pub fn grt_gwei_string_to_f32(input: &str) -> Result<f32, QueryError> {
    add_decimal(input)
        .parse::<f32>()
        .map_err(|e| QueryError::ParseResponseError(e.to_string()))
}

pub fn add_decimal(input: &str) -> String {
    if input.len() <= 18 {
        return format!("0.{}", input);
    }
    let (left, right) = input.split_at(input.len() - 18);
    format!("{}.{}", left, right)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_add_decimal() {
        // valid inputs
        assert_eq!(
            add_decimal("100000000000000000000000"),
            "100000.000000000000000000",
        );
        assert_eq!(add_decimal("0"), "0.0");
        assert_eq!(
            add_decimal("30921273477321769415119223"),
            "30921273.477321769415119223",
        );
    }

    #[test]
    fn test_grt_gwei_string_to_f32() {
        // valid inputs
        assert!(grt_gwei_string_to_f32("100000000000000000000000").is_ok());
        assert_eq!(
            grt_gwei_string_to_f32("100000000000000000000000").unwrap(),
            100000.0,
        );
        assert!(grt_gwei_string_to_f32("0").is_ok());
        assert!(grt_gwei_string_to_f32("30921273477321769415119223").is_ok());
        assert_eq!(
            grt_gwei_string_to_f32("30921273477321769415119223").unwrap(),
            30921273.477321769415119223,
        );

        // invalid inputs
        assert!(grt_gwei_string_to_f32("abc").is_err(),);
    }
}
