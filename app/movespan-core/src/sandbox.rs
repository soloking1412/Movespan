//! Mode B: an in-memory Aptos chain used to capture access sets from a
//! synthetic workload, with no real network or users.

use anyhow::{anyhow, Result};
use aptos_language_e2e_tests::account::Account;
use aptos_language_e2e_tests::executor::FakeExecutor;
use aptos_types::state_store::state_key::StateKey;
use aptos_types::transaction::{
    ExecutionStatus, SignedTransaction, Transaction, TransactionOutput, TransactionStatus,
};
use move_binary_format::CompiledModule;
use move_core_types::account_address::AccountAddress;
use move_core_types::language_storage::ModuleId;

use movespan_types::{LocationTable, TxnAccess};

use crate::recorder::RecordingStateView;
use crate::state_key::LocationInterner;

/// Balance funded to each simulated user account. Generous because gas is
/// priced at zero here; the balance only needs to cover any value the workload
/// itself moves.
const USER_BALANCE: u64 = 1_000_000_000_000;

/// A genesis-seeded in-memory executor plus the accounts and location registry
/// built up while running a workload.
pub struct Sandbox {
    executor: FakeExecutor,
    accounts: Vec<Account>,
    interner: LocationInterner,
}

impl Sandbox {
    /// A fresh chain with the head framework already deployed.
    pub fn new() -> Self {
        Self::from_executor(FakeExecutor::from_head_genesis())
    }

    /// Wrap an existing executor, e.g. one forked from remote state.
    pub fn from_executor(executor: FakeExecutor) -> Self {
        Self {
            executor,
            accounts: Vec::new(),
            interner: LocationInterner::new(),
        }
    }

    /// Create and fund `n` simulated users. Contention arises from many of them
    /// touching the same state, which is exactly what the profiler measures.
    pub fn create_accounts(&mut self, n: usize) -> Vec<Account> {
        let accounts = self.executor.create_accounts(n, USER_BALANCE, 0);
        self.accounts = accounts.clone();
        accounts
    }

    pub fn accounts(&self) -> &[Account] {
        &self.accounts
    }

    /// Create a funded account at a fixed address so it can own the published
    /// package and run its initializers.
    pub fn account_at(&mut self, address: AccountAddress) -> Account {
        self.executor.new_account_at(address)
    }

    /// Publish a compiled module's bytecode, returning its module id.
    pub fn publish_module(&mut self, code: Vec<u8>) -> Result<ModuleId> {
        let module = CompiledModule::deserialize(&code)
            .map_err(|e| anyhow!("failed to deserialize module: {e:?}"))?;
        let id = module.self_id();
        self.executor.add_module(&id, code);
        Ok(id)
    }

    /// Run a setup transaction and apply it, without recording access sets.
    pub fn run(&mut self, txn: SignedTransaction) -> Result<TransactionOutput> {
        let output = self.executor.execute_and_apply(txn);
        match output.status() {
            TransactionStatus::Keep(ExecutionStatus::Success) => Ok(output),
            other => Err(anyhow!("setup transaction failed: {other:?}")),
        }
    }

    /// Execute one transaction, capture its read and write sets, and apply its
    /// writes so the next transaction observes them in block order.
    pub fn run_and_capture(&mut self, txn: SignedTransaction) -> Result<TxnAccess> {
        let block = vec![Transaction::UserTransaction(txn)];

        let (reads, output) = {
            let base = self.executor.state_store();
            let recorder = RecordingStateView::new(base);
            let outputs = self
                .executor
                .execute_transaction_block_with_state_view(block, &recorder, true)
                .map_err(|status| anyhow!("VM execution failed: {status:?}"))?;
            let output = outputs
                .into_iter()
                .next()
                .ok_or_else(|| anyhow!("no transaction output produced"))?;
            (recorder.take_reads(), output)
        };

        let success = matches!(
            output.status(),
            TransactionStatus::Keep(ExecutionStatus::Success)
        );
        let write_keys: Vec<StateKey> = output
            .write_set()
            .write_op_iter()
            .map(|(key, _)| key.clone())
            .collect();
        let gas_used = output.gas_used();

        // Persist this transaction's effects (including a failed transaction's
        // gas and sequence-number bump) so ordering is faithful to a block.
        self.executor.apply_write_set(output.write_set());

        let reads = reads.iter().map(|key| self.interner.intern(key)).collect();
        let writes = write_keys
            .iter()
            .map(|key| self.interner.intern(key))
            .collect();

        Ok(TxnAccess {
            reads,
            writes,
            gas_used,
            success,
        })
    }

    /// Snapshot the location registry built up so far.
    pub fn locations(&self) -> LocationTable {
        self.interner.table().clone()
    }
}

impl Default for Sandbox {
    fn default() -> Self {
        Self::new()
    }
}
