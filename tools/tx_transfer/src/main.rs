use ethers::prelude::*;
use k256::pkcs8::der::Encode;
use log::{error, info};
use serde::Deserialize;
use std::{fs, sync::Arc};
use tokio::sync::mpsc;

#[derive(Deserialize)]
struct Config {
    ethereum: EthereumConfig,
    sidechain: SidechainConfig,
    filter: FilterConfig,
    daconfig: da_service::DaServiceConfig,
}

#[derive(Deserialize)]
struct EthereumConfig {
    rpc_url: String,
    start_height: u64,
}

#[derive(Deserialize)]
struct SidechainConfig {
    rpc_url: String,
}

#[derive(Deserialize)]
struct FilterConfig {
    target_address: String,
}

pub mod da_service;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::init();

    let config: Config = toml::from_str(&fs::read_to_string("config.toml")?)?;
    info!("Loaded configuration");

    let provider = Provider::<Http>::try_from(config.ethereum.rpc_url.clone())?;
    let provider = Arc::new(provider);

    let sidechain_provider = Provider::<Http>::try_from(config.sidechain.rpc_url.clone())?;
    let _sidechain_provider = Arc::new(sidechain_provider);

    let da_service = da_service::CelestiaService::new(config.daconfig).await;

    let (tx, mut rx) = mpsc::channel(100);

    let provider_clone = provider.clone();
    let _filter_target = config.filter.target_address.clone();

    tokio::spawn(async move {
        // if let Err(e) = listen_ethereum_transactions(provider_clone, filter_target, tx).await {
        //     error!("Error while listening to Ethereum transactions: {:?}", e);
        // }
        if let Err(e) =
            process_blocks_from_height(provider_clone, config.ethereum.start_height, None, tx).await
        {
            error!("Error while listening to Ethereum transactions: {:?}", e);
        }
    });

    while let Some(transaction) = rx.recv().await {
        // if let Err(e) = forward_to_sidechain(sidechain_provider.clone(), transaction).await {
        //     error!("Error while forwarding transaction: {:?}", e);
        // }
        if let Err(e) = forward_to_da(da_service.clone(), transaction).await {
            error!("Error while forwarding transaction: {:?}", e);
        }
    }

    Ok(())
}

#[allow(dead_code)]
async fn listen_ethereum_transactions(
    provider: Arc<Provider<Http>>,
    target_address: String,
    tx_sender: mpsc::Sender<Transaction>,
) -> anyhow::Result<()> {
    let block_stream = provider.watch_blocks().await?;
    let mut block_stream = block_stream.stream();

    while let Some(block_hash) = block_stream.next().await {
        info!("Received new block: {:?}", block_hash);
        if let Ok(Some(block)) = provider.get_block_with_txs(block_hash).await {
            info!("Received block with transactions: {:?}", block);
            for tx in block.transactions {
                if tx
                    .to
                    .map(|to| to == target_address.parse().unwrap())
                    .unwrap_or(false)
                {
                    info!("Filtered transaction: {:?}", tx);
                    tx_sender.send(tx).await.map_err(|e| anyhow::anyhow!(e))?;
                }
            }
        }
    }

    Ok(())
}

#[allow(dead_code)]
pub async fn process_blocks_from_height(
    provider: Arc<Provider<Http>>,
    start_height: u64,
    target_address: Option<H160>,
    tx_sender: mpsc::Sender<Transaction>,
) -> anyhow::Result<()> {
    let mut current_height = start_height;

    loop {
        match provider.get_block_with_txs(current_height).await {
            Ok(Some(block)) => {
                info!(
                    "Processing block number: {} txs: {}",
                    current_height,
                    block.transactions.len(),
                );
                for tx in block.transactions {
                    if let Some(target) = target_address {
                        if tx.to != Some(target) {
                            continue;
                        }
                    }
                    info!("Forwarding transaction: {:?}", tx);
                    tx_sender.send(tx).await.map_err(|e| anyhow::anyhow!(e))?;
                }
                current_height += 1;
            }
            Ok(None) => {
                info!(
                    "Block at height {} not found yet. Retrying...",
                    current_height,
                );
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            }
            Err(e) => {
                info!("Error fetching block at height {}: {:?}", current_height, e);
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            }
        }
    }
}

#[allow(dead_code)]
async fn forward_to_sidechain(
    provider: Arc<Provider<Http>>,
    transaction: Transaction,
) -> anyhow::Result<()> {
    let tx_request = TransactionRequest::new()
        .from(transaction.from)
        .to(transaction.to.unwrap())
        .value(transaction.value)
        .data(transaction.input)
        .gas(21000)
        .gas_price(1_000_000_000u64);

    let pending_tx = provider.send_transaction(tx_request, None).await?;
    info!("Forwarded transaction with hash: {:?}", pending_tx);

    Ok(())
}

#[allow(dead_code)]
async fn forward_to_da(
    provider: da_service::CelestiaService,
    transaction: Transaction,
) -> anyhow::Result<()> {
    // let tx_request = TransactionRequest::new()
    //     .from(transaction.from)
    //     .to(transaction.to.unwrap())
    //     .value(transaction.value)
    //     .data(transaction.input)
    //     .gas(21000)
    //     .gas_price(1_000_000_000u64);

    let block_json = serde_json::to_string(&transaction)?;
    let pending_tx = provider
        .send_transaction(block_json.as_bytes())
        .await
        .unwrap();
    info!("Forwarded transaction with hash: {:?}", pending_tx);

    Ok(())
}
