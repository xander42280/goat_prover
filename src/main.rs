use common::file;
use ethers_providers::{Http, Provider};
use models::TestUnit;
use std::env;
use std::fs::read;
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;
use zkm_sdk::{prover::ProverInput, ProverClient};

async fn prove(
    json_path: &str,
    elf_path: &str,
    seg_size: u32,
    execute_only: bool,
    outdir: &str,
    block_no: u64,
) {
    log::info!("Start prove block! block_no:{}", block_no);
    let prover_client = ProverClient::new().await;
    let input = ProverInput {
        elf: read(elf_path).unwrap(),
        public_inputstream: read(json_path).unwrap(),
        private_inputstream: "".into(),
        seg_size,
        execute_only,
        args: "".into(),
    };

    let start = Instant::now();
    let proving_result = prover_client.prover.prove(&input, None).await;
    match proving_result {
        Ok(Some(prover_result)) => {
            if !execute_only {
                if prover_result.proof_with_public_inputs.is_empty() {
                    log::info!(
                        "Fail: snark_proof_with_public_inputs.len() is : {}.Please try setting SEG_SIZE={}",
                        prover_result.proof_with_public_inputs.len(), seg_size/2
                    );
                }
                let output_path = Path::new(outdir);
                let proof_result_path =
                    output_path.join(format!("{}_snark_proof_with_public_inputs.json", block_no));
                let mut f = file::new(&proof_result_path.to_string_lossy());
                match f.write(prover_result.proof_with_public_inputs.as_slice()) {
                    Ok(bytes_written) => {
                        log::info!("Proof: successfully written {} bytes.", bytes_written);
                    }
                    Err(e) => {
                        log::info!("Proof: failed to write to file: {}", e);
                    }
                }
                log::info!("Generating proof successfully.");
            } else {
                log::info!("Generating proof successfully .The proof is not saved.");
            }
        }
        Ok(None) => {
            log::info!("Failed to generate proof.The result is None.");
        }
        Err(e) => {
            log::info!("Failed to generate proof. error: {}", e);
        }
    }

    let end = Instant::now();
    let elapsed = end.duration_since(start);
    log::info!(
        "Elapsed time: {:?} secs block_no:{}",
        elapsed.as_secs(),
        block_no
    );
}

async fn generate_json_file(
    client: Arc<Provider<Http>>,
    block_no: u64,
    chain_id: u64,
    dir: &str,
) -> anyhow::Result<(String, TestUnit)> {
    let json_string = executor::process(client, block_no, chain_id).await?;
    // only execute the block if has transactions
    let test_unit = serde_json::from_str::<models::TestUnit>(&json_string)?;
    if !test_unit.pre.is_empty() || !test_unit.post.is_empty() {
        let mut buf = Vec::new();
        bincode::serialize_into(&mut buf, &json_string)?;
        let suite_json_path = format!("{}/{}.json", dir, block_no);
        std::fs::write(suite_json_path.clone(), buf)?;
        Ok((suite_json_path, test_unit))
    } else {
        Ok(("".to_string(), test_unit))
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::try_init().unwrap_or_default();
    let block_no = env::var("BLOCK_NO").unwrap_or(String::from("1"));
    let mut block_no: u64 = block_no.parse().unwrap();
    let rpc_url = env::var("RPC_URL").unwrap_or(String::from("http://localhost:8545"));
    let chain_id = env::var("CHAIN_ID").unwrap_or(String::from("1"));
    let output_dir = env::var("OUTPUT_DIR").unwrap_or(String::from("./output"));
    let seg_size = env::var("SEG_SIZE").unwrap_or("65536".to_string());
    let seg_size = seg_size.parse::<_>().unwrap_or(65536);
    let execute_only = env::var("EXECUTE_ONLY").unwrap_or("false".to_string());
    let execute_only = execute_only.parse::<bool>().unwrap_or(false);
    let elf_path = env::var("ELF_PATH").expect("ELF PATH is missed");

    let client = Provider::<Http>::try_from(rpc_url).unwrap();
    let client = Arc::new(client);

    loop {
        let ret = generate_json_file(
            client.clone(),
            block_no,
            chain_id.parse().unwrap(),
            &output_dir,
        )
        .await;
        match ret {
            Ok((json_file_path, test_unit)) => {
                log::info!(
                    "Generating json file for block_no: {} is successful",
                    block_no
                );
                if json_file_path.is_empty() {
                    log::info!("Block_no: {} has no transactions", block_no);
                } else {
                    let start_time = Instant::now();
                    prove(
                        &json_file_path,
                        &elf_path,
                        seg_size,
                        execute_only,
                        &output_dir,
                        block_no,
                    )
                    .await;
                    let end_time = Instant::now();
                    log::info!(
                        "Elapsed time: {};{};{}",
                        block_no,
                        test_unit.env.parent_blob_gas_used.unwrap_or_default(),
                        end_time.duration_since(start_time).as_secs(),
                    );
                }
                block_no += 1;
            }
            Err(e) => {
                log::error!("Generating json file for block_no: {} is failed", block_no);
                log::error!("Error: {}", e);
                tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;
                continue;
            }
        }
    }
}
