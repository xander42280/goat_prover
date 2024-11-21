use ethers::prelude::*;
use log::{error, info};
use serde::Deserialize;
use std::{fs, sync::Arc};
use tokio::sync::mpsc;

#[derive(Deserialize)]
struct Config {
    ethereum: EthereumConfig,
    sidechain: SidechainConfig,
    filter: FilterConfig,
}

#[derive(Deserialize)]
struct EthereumConfig {
    rpc_url: String,
}

#[derive(Deserialize)]
struct SidechainConfig {
    rpc_url: String,
}

#[derive(Deserialize)]
struct FilterConfig {
    target_address: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::init();

    let config: Config = toml::from_str(&fs::read_to_string("config.toml")?)?;
    info!("Loaded configuration");

    let provider = Provider::<Http>::try_from(config.ethereum.rpc_url.clone())?;
    let provider = Arc::new(provider);

    let sidechain_provider = Provider::<Http>::try_from(config.sidechain.rpc_url.clone())?;
    let sidechain_provider = Arc::new(sidechain_provider);

    let (tx, mut rx) = mpsc::channel(100);

    let provider_clone = provider.clone();
    let filter_target = config.filter.target_address.clone();

    tokio::spawn(async move {
        if let Err(e) = listen_ethereum_transactions(provider_clone, filter_target, tx).await {
            error!("Error while listening to Ethereum transactions: {:?}", e);
        }
    });

    while let Some(transaction) = rx.recv().await {
        if let Err(e) = forward_to_sidechain(sidechain_provider.clone(), transaction).await {
            error!("Error while forwarding transaction: {:?}", e);
        }
    }

    Ok(())
}

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

async fn forward_to_sidechain(
    provider: Arc<Provider<Http>>,
    transaction: Transaction,
) -> anyhow::Result<()> {
    let tx_request = TransactionRequest::new()
        .to(transaction.to.unwrap())
        .value(transaction.value)
        .data(transaction.input);

    let pending_tx = provider.send_transaction(tx_request, None).await?;
    info!("Forwarded transaction with hash: {:?}", pending_tx);

    Ok(())
}
