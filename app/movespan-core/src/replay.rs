//! Mode A: fork live network state and re-execute historical transactions,
//! capturing their access sets to profile real traffic.

use anyhow::{anyhow, Result};
use aptos_language_e2e_tests::executor::FakeExecutor;
use aptos_rest_client::{AptosBaseUrl, Client};
use aptos_types::transaction::{SignedTransaction, Transaction};
use movespan_types::Workload;
use url::Url;

use crate::sandbox::Sandbox;

/// Which network to fork and read history from.
#[derive(Debug, Clone)]
pub enum Network {
    Mainnet,
    Testnet,
    Devnet,
    Custom(String),
}

impl Network {
    fn base_url(&self) -> Result<AptosBaseUrl> {
        Ok(match self {
            Network::Mainnet => AptosBaseUrl::Mainnet,
            Network::Testnet => AptosBaseUrl::Testnet,
            Network::Devnet => AptosBaseUrl::Devnet,
            Network::Custom(url) => {
                AptosBaseUrl::Custom(Url::parse(url).map_err(|e| anyhow!("invalid node url: {e}"))?)
            }
        })
    }

    fn client(&self) -> Result<Client> {
        Ok(Client::builder(self.base_url()?).build())
    }
}

/// Range of committed transactions to replay.
#[derive(Debug, Clone)]
pub struct ReplayConfig {
    pub network: Network,
    pub start_version: u64,
    pub count: u64,
}

/// Fetch `count` user transactions from `start_version`, replay them against
/// forked state, and return the captured workload.
pub fn replay(config: &ReplayConfig) -> Result<Workload> {
    let client = config.network.client()?;
    let txns = fetch_user_transactions(&client, config.start_version, config.count)?;

    let executor =
        FakeExecutor::from_remote_state(config.network.base_url()?, config.start_version);
    let mut sandbox = Sandbox::from_executor(executor);

    let mut accesses = Vec::with_capacity(txns.len());
    for txn in txns {
        accesses.push(sandbox.run_and_capture(txn)?);
    }

    Ok(Workload {
        txns: accesses,
        locations: sandbox.locations(),
    })
}

fn fetch_user_transactions(
    client: &Client,
    start_version: u64,
    count: u64,
) -> Result<Vec<SignedTransaction>> {
    const PAGE: u16 = 1000;

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;

    runtime.block_on(async {
        let mut collected = Vec::new();
        let mut cursor = start_version;

        while (collected.len() as u64) < count {
            let page = client
                .get_transactions_bcs(Some(cursor), Some(PAGE))
                .await
                .map_err(|e| anyhow!("failed to fetch transactions: {e}"))?
                .into_inner();
            if page.is_empty() {
                break;
            }
            for onchain in &page {
                cursor = onchain.version + 1;
                if let Transaction::UserTransaction(signed) = &onchain.transaction {
                    collected.push(signed.clone());
                }
            }
        }

        collected.truncate(count as usize);
        Ok(collected)
    })
}
