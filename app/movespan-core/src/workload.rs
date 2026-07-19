//! Mode B workload specification and the deterministic generator that expands
//! it into signed entry-function transactions.

use anyhow::{anyhow, bail, Result};
use aptos_language_e2e_tests::account::Account;
use aptos_types::transaction::{EntryFunction, SignedTransaction};
use move_core_types::account_address::AccountAddress;
use move_core_types::identifier::Identifier;
use move_core_types::language_storage::ModuleId;
use serde::Deserialize;

/// A workload description, deserialized from TOML.
#[derive(Debug, Deserialize)]
pub struct WorkloadSpec {
    /// Number of simulated users.
    pub accounts: usize,
    /// Total measured transactions to generate.
    pub txns: usize,
    /// Seed for reproducible runs.
    #[serde(default)]
    pub seed: u64,
    /// One-time setup calls, run by the package account before measurement.
    #[serde(default)]
    pub init: Vec<CallSpec>,
    /// Measured calls, drawn by weight.
    pub calls: Vec<CallSpec>,
}

/// A single entry-function call template.
#[derive(Debug, Clone, Deserialize)]
pub struct CallSpec {
    /// Fully qualified function, `0xADDR::module::function`.
    pub function: String,
    #[serde(default = "default_weight")]
    pub weight: u32,
    #[serde(default)]
    pub args: Vec<ArgSpec>,
}

fn default_weight() -> u32 {
    1
}

/// An argument generator. Constants force shared access (contention); random
/// values spread it out.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ArgSpec {
    ConstU64 { value: u64 },
    RandU64 { min: u64, max: u64 },
    ConstAddress { value: String },
    RandAccount,
}

/// The transaction builder defaults the gas price to zero, which the VM rejects
/// with GAS_UNIT_PRICE_BELOW_MIN_BOUND, so it is set explicitly to clear the gas
/// schedule's minimum.
const GAS_UNIT_PRICE: u64 = 100;

/// Ample for entry-function workloads while leaving accounts funded well above
/// `MAX_GAS_AMOUNT * GAS_UNIT_PRICE`.
const MAX_GAS_AMOUNT: u64 = 1_000_000;

/// A generated transaction and the call template that produced it.
pub struct PlannedTxn {
    pub txn: SignedTransaction,
    pub function: String,
}

/// Expand the measured `calls` into `spec.txns` signed transactions spread
/// across `accounts`.
pub fn plan_calls(spec: &WorkloadSpec, accounts: &[Account]) -> Result<Vec<PlannedTxn>> {
    if accounts.len() < spec.accounts {
        bail!(
            "workload needs {} accounts but only {} were created",
            spec.accounts,
            accounts.len()
        );
    }
    if spec.accounts == 0 {
        bail!("workload must use at least one account");
    }
    if spec.calls.is_empty() {
        bail!("workload has no measured calls");
    }

    let templates: Vec<Template> = spec
        .calls
        .iter()
        .map(Template::parse)
        .collect::<Result<_>>()?;
    let total_weight: u64 = templates.iter().map(|t| u64::from(t.weight)).sum();

    let mut rng = Lcg::new(spec.seed ^ 0x9E37_79B9_7F4A_7C15);
    let mut sequence_numbers = vec![0u64; spec.accounts];
    let mut planned = Vec::with_capacity(spec.txns);

    for _ in 0..spec.txns {
        let template = pick_weighted(&templates, total_weight, &mut rng);
        let sender_index = (rng.next() as usize) % spec.accounts;
        let sequence_number = sequence_numbers[sender_index];
        sequence_numbers[sender_index] += 1;

        let txn = build_txn(
            &accounts[sender_index],
            sequence_number,
            template,
            accounts,
            &mut rng,
        )?;
        planned.push(PlannedTxn {
            txn,
            function: template.label.clone(),
        });
    }

    Ok(planned)
}

/// Build the one-time initialization transactions run by `account`.
pub fn plan_init(
    spec: &WorkloadSpec,
    account: &Account,
    accounts: &[Account],
) -> Result<Vec<SignedTransaction>> {
    let mut rng = Lcg::new(spec.seed ^ 0xD1B5_4A32_D192_ED03);
    spec.init
        .iter()
        .enumerate()
        .map(|(i, call)| {
            let template = Template::parse(call)?;
            build_txn(account, i as u64, &template, accounts, &mut rng)
        })
        .collect()
}

struct Template {
    module: ModuleId,
    function: Identifier,
    args: Vec<ArgSpec>,
    weight: u32,
    label: String,
}

impl Template {
    fn parse(call: &CallSpec) -> Result<Self> {
        let parts: Vec<&str> = call.function.split("::").collect();
        if parts.len() != 3 {
            bail!(
                "function must be '0xADDR::module::function', got '{}'",
                call.function
            );
        }
        let address = AccountAddress::from_hex_literal(parts[0])
            .map_err(|e| anyhow!("invalid address in '{}': {e}", call.function))?;
        let module = ModuleId::new(address, Identifier::new(parts[1])?);
        let function = Identifier::new(parts[2])?;
        Ok(Self {
            module,
            function,
            args: call.args.clone(),
            weight: call.weight.max(1),
            label: call.function.clone(),
        })
    }
}

fn pick_weighted<'a>(templates: &'a [Template], total_weight: u64, rng: &mut Lcg) -> &'a Template {
    let mut pick = rng.next() % total_weight;
    for template in templates {
        let weight = u64::from(template.weight);
        if pick < weight {
            return template;
        }
        pick -= weight;
    }
    templates.last().expect("templates is non-empty")
}

fn build_txn(
    sender: &Account,
    sequence_number: u64,
    template: &Template,
    accounts: &[Account],
    rng: &mut Lcg,
) -> Result<SignedTransaction> {
    let args = template
        .args
        .iter()
        .map(|arg| encode_arg(arg, accounts, rng))
        .collect::<Result<Vec<_>>>()?;
    let entry = EntryFunction::new(
        template.module.clone(),
        template.function.clone(),
        Vec::new(),
        args,
    );
    Ok(sender
        .transaction()
        .entry_function(entry)
        .sequence_number(sequence_number)
        .max_gas_amount(MAX_GAS_AMOUNT)
        .gas_unit_price(GAS_UNIT_PRICE)
        .sign())
}

fn encode_arg(arg: &ArgSpec, accounts: &[Account], rng: &mut Lcg) -> Result<Vec<u8>> {
    let bytes = match arg {
        ArgSpec::ConstU64 { value } => bcs::to_bytes(value)?,
        ArgSpec::RandU64 { min, max } => bcs::to_bytes(&rng.range(*min, *max))?,
        ArgSpec::ConstAddress { value } => {
            let address = AccountAddress::from_hex_literal(value)
                .map_err(|e| anyhow!("invalid address '{value}': {e}"))?;
            bcs::to_bytes(&address)?
        }
        ArgSpec::RandAccount => {
            let index = (rng.next() as usize) % accounts.len();
            bcs::to_bytes(accounts[index].address())?
        }
    };
    Ok(bytes)
}

/// A small linear congruential generator, kept local so runs are reproducible
/// from the workload seed alone.
struct Lcg(u64);

impl Lcg {
    fn new(seed: u64) -> Self {
        Self(seed)
    }

    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0
    }

    fn range(&mut self, min: u64, max: u64) -> u64 {
        if max <= min {
            return min;
        }
        min + self.next() % (max - min)
    }
}
