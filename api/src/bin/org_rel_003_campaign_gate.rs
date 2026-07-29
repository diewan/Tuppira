//! Observation-only ingestion/output gate for the ORG-REL-003 campaign.

use serde::Deserialize;
use serde_json::json;
use std::{env, fs};

#[derive(Deserialize)]
struct Campaign {
    schema: String,
    reference_chain: ReferenceChain,
    isolated_successors: Vec<Successor>,
    closure: Closure,
}

#[derive(Deserialize)]
struct ReferenceChain {
    finalized_checkpoint: Checkpoint,
}

#[derive(Deserialize)]
struct Checkpoint {
    block_hash: String,
    block_height: u64,
    observed_tip_height: u64,
    confirmations_required: u64,
}

#[derive(Deserialize)]
struct Successor {
    txid: String,
}

#[derive(Deserialize)]
struct Closure {
    valid_successor: String,
    valid_successor_count: u64,
}

fn main() -> Result<(), String> {
    let path = env::args_os()
        .nth(1)
        .ok_or("usage: org-rel-003-campaign-gate RESULT")?;
    let campaign: Campaign = serde_json::from_slice(
        &fs::read(path).map_err(|error| format!("campaign result: {error}"))?,
    )
    .map_err(|error| format!("campaign result: {error}"))?;
    let checkpoint = &campaign.reference_chain.finalized_checkpoint;
    if campaign.schema != "diewan.org-rel-003.result.v1"
        || campaign.isolated_successors.len() != 2
        || campaign.closure.valid_successor_count != 1
        || checkpoint.observed_tip_height < checkpoint.block_height
        || checkpoint.confirmations_required == 0
    {
        return Err("campaign observation is incomplete".into());
    }
    let transaction_ids = campaign
        .isolated_successors
        .iter()
        .map(|successor| successor.txid.clone())
        .collect::<Vec<_>>();
    if transaction_ids[0] == transaction_ids[1]
        || !transaction_ids.contains(&campaign.closure.valid_successor)
    {
        return Err("campaign successor identities are inconsistent".into());
    }

    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "schema": "diewan.tuppira.org-rel-003.observation.v1",
            "attempts_displayed": transaction_ids,
            "finalized_observation": {
                "block_hash": checkpoint.block_hash,
                "block_height": checkpoint.block_height,
                "observed_tip_height": checkpoint.observed_tip_height
            },
            "campaign_reported_valid_successor": campaign.closure.valid_successor,
            "authority": "observation-only"
        }))
        .map_err(|error| error.to_string())?
    );
    Ok(())
}
