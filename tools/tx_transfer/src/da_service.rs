use celestia_rpc::prelude::*;
use celestia_types::blob::{Blob as JsonBlob, Commitment, SubmitOptions};
use celestia_types::consts::appconsts::{
    CONTINUATION_SPARSE_SHARE_CONTENT_SIZE, FIRST_SPARSE_SHARE_CONTENT_SIZE, SHARE_SIZE,
};
use celestia_types::nmt::Namespace;
use ethers::core::k256::sha2::digest::block_buffer::Error;
use jsonrpsee::http_client::{HeaderMap, HttpClient};
use log::{error, info};

#[derive(Debug, Clone)]
pub struct CelestiaService {
    client: HttpClient,
    rollup_namespace: Namespace,
}

impl CelestiaService {
    pub fn with_client(client: HttpClient, nid: Namespace) -> Self {
        Self {
            client,
            rollup_namespace: nid,
        }
    }
}

/// Runtime configuration for the DA service
#[derive(Debug, Clone, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct DaServiceConfig {
    /// The jwt used to authenticate with the Celestia rpc server
    pub celestia_rpc_auth_token: String,
    pub namespace: Namespace,
    /// The address of the Celestia rpc server
    #[serde(default = "default_rpc_addr")]
    pub celestia_rpc_address: String,
    /// The maximum size of a Celestia RPC response, in bytes
    #[serde(default = "default_max_response_size")]
    pub max_celestia_response_body_size: u32,
    /// The timeout for a Celestia RPC request, in seconds
    #[serde(default = "default_request_timeout_seconds")]
    pub celestia_rpc_timeout_seconds: u64,
}

fn default_rpc_addr() -> String {
    "http://localhost:11111/".into()
}

fn default_max_response_size() -> u32 {
    1024 * 1024 * 100 // 100 MB
}

const fn default_request_timeout_seconds() -> u64 {
    60
}

const GAS_PER_BYTE: usize = 20;
const GAS_PRICE: usize = 1;

impl CelestiaService {
    pub async fn new(config: DaServiceConfig) -> Self {
        let client = {
            let mut headers = HeaderMap::new();
            headers.insert(
                "Authorization",
                format!("Bearer {}", config.celestia_rpc_auth_token)
                    .parse()
                    .unwrap(),
            );

            jsonrpsee::http_client::HttpClientBuilder::default()
                .set_headers(headers)
                .max_request_size(config.max_celestia_response_body_size)
                .request_timeout(std::time::Duration::from_secs(
                    config.celestia_rpc_timeout_seconds,
                ))
                .build(&config.celestia_rpc_address)
        }
        .expect("Client initialization is valid");

        Self::with_client(client, config.namespace)
    }

    pub async fn send_transaction(&self, blob: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
        info!("Sending {} bytes of raw data to Celestia.", blob.len());

        let gas_limit = get_gas_limit_for_bytes(blob.len()) as u64;
        let fee = gas_limit * GAS_PRICE as u64;

        let blob = JsonBlob::new(self.rollup_namespace, blob.to_vec())?;
        info!("Submiting: {:?}", blob.commitment);

        let height = self
            .client
            .blob_submit(
                &[blob],
                SubmitOptions {
                    fee: Some(fee),
                    gas_limit: Some(gas_limit),
                },
            )
            .await?;
        info!(
            "Blob has been submitted to Celestia. block-height={}",
            height,
        );
        Ok(())
    }
}

// https://docs.celestia.org/learn/submit-data/#fees-and-gas-limits
fn get_gas_limit_for_bytes(n: usize) -> usize {
    let fixed_cost = 75000;

    let continuation_shares_needed =
        n.saturating_sub(FIRST_SPARSE_SHARE_CONTENT_SIZE) / CONTINUATION_SPARSE_SHARE_CONTENT_SIZE;
    let shares_needed = 1 + continuation_shares_needed + 1; // add one extra, pessimistic

    fixed_cost + shares_needed * SHARE_SIZE * GAS_PER_BYTE
}
