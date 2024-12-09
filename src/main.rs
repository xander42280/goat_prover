use common::file;
use ethers_providers::{Http, Provider};
use std::env;
use std::fs::read;
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;
use zkm_sdk::{prover::ClientCfg, prover::ProverInput, ProverClient};

mod check;

async fn prove(
    cfg: &ClientCfg,
    json_path: &str,
    elf_path: &str,
    seg_size: u32,
    execute_only: bool,
    outdir: &str,
    block_no: u64,
) {
    log::info!("Start prove block! block_no:{}", block_no);
    let prover_client = ProverClient::new(cfg).await;
    let input = ProverInput {
        elf: read(elf_path).unwrap(),
        public_inputstream: read(json_path).unwrap(),
        private_inputstream: vec![],
        seg_size,
        execute_only,
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

async fn prove_tx(
    cfg: &ClientCfg,
    outdir: &str,
    elf_path: &str,
    seg_size: u32,
    execute_only: bool,
    test_suite: &models::TestSuite,
    block_no: u64,
) -> anyhow::Result<()> {
    let mut buf = Vec::new();
    let json_string = serde_json::to_string(&test_suite).expect("Failed to serialize");
    log::debug!("test_suite: {}", json_string);
    bincode::serialize_into(&mut buf, &json_string).expect("serialization failed");
    let suite_json_path = format!("{}/{}.json", outdir, block_no);
    std::fs::write(suite_json_path.clone(), &buf)?;
    let check_start_time = Instant::now();
    crate::check::execute_test_suite(&buf).unwrap();
    let check_end_time = Instant::now();
    log::info!(
        "Elapsed time: {:?} micros check block_no:{}",
        check_end_time.duration_since(check_start_time).as_micros(),
        block_no
    );
    if elf_path.is_empty() {
        log::info!("ELF_PATH is empty, skip proving");
        return Ok(());
    }
    let start_time = Instant::now();
    prove(
        cfg,
        &suite_json_path,
        elf_path,
        seg_size,
        execute_only,
        outdir,
        block_no,
    )
    .await;
    let end_time = Instant::now();
    log::info!(
        "Elapsed time: {};{};{};{}",
        block_no,
        test_suite.0.len(),
        test_suite
            .0
            .first_key_value()
            .unwrap()
            .1
            .env
            .parent_blob_gas_used
            .unwrap_or_default(),
        end_time.duration_since(start_time).as_secs(),
    );

    Ok(())
}

async fn check(filepath: &str) -> anyhow::Result<()> {
    let buf = std::fs::read(filepath).expect("Failed to read file");
    crate::check::execute_test_suite(&buf).unwrap();
    Ok(())
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
    let elf_path = env::var("ELF_PATH").unwrap_or("".to_string());
    let endpoint = env::var("ENDPOINT").ok();
    let ca_cert_path = env::var("CA_CERT_PATH").ok();
    let cert_path = env::var("CERT_PATH").ok();
    let key_path = env::var("KEY_PATH").ok();
    let domain_name = env::var("DOMAIN_NAME").ok();
    let private_key = env::var("PRIVATE_KEY").ok();
    let prove_loop = env::var("PROVE_LOOP").unwrap_or("false".to_string());
    let prove_loop = prove_loop.parse::<bool>().unwrap_or(false);

    let args: Vec<String> = env::args().collect();
    if args.len() > 2 {
        match args[1].as_str() {
            "check" => check(args[2].as_str()).await?,
            &_ => todo!(),
        };
        return Ok(());
    }

    let client = Provider::<Http>::try_from(rpc_url).unwrap();
    let client = Arc::new(client);

    let prover_cfg = ClientCfg {
        zkm_prover: env::var("ZKM_PROVER").unwrap_or(String::from("network")),
        vk_path: env::var("VK_PATH").unwrap_or(String::from("")),
        endpoint,
        ca_cert_path,
        cert_path,
        key_path,
        domain_name,
        private_key,
    };

    loop {
        let test_suite =
            executor::process(client.clone(), block_no, chain_id.parse().unwrap()).await;
        match test_suite {
            anyhow::Result::Ok(items) => {
                log::info!(
                    "Generating json file for block_no: {} is successful, txs: {}",
                    block_no,
                    items.0.len(),
                );

                if !items.0.is_empty() {
                    prove_tx(
                        &prover_cfg,
                        &output_dir,
                        &elf_path,
                        seg_size,
                        execute_only,
                        &items,
                        block_no,
                    )
                    .await?;
                }
                block_no += 1;
            }
            Err(e) => {
                log::error!("Generating json file for block_no: {} is failed", block_no);
                log::error!("Error: {}", e);
                tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;
            }
        }

        if !prove_loop {
            break;
        }
    }
    Ok(())
}
