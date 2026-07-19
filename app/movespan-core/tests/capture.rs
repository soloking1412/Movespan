//! Phase-1 capture gate: proves that read/write sets are recorded from the real
//! VM. It requires the aptos-core dependencies to be built and the `counter`
//! fixture compiled with `counter=0xC0FFEE`, so it is ignored by default. Run:
//!
//! ```sh
//! (cd fixtures/counter && aptos move compile --named-addresses counter=0xC0FFEE)
//! cargo test -p movespan-core -- --ignored
//! ```

use std::collections::HashSet;
use std::path::PathBuf;

use move_core_types::account_address::AccountAddress;

use movespan_core::{plan_calls, plan_init, Sandbox, WorkloadSpec};
use movespan_types::KeyClass;

const COUNTER_ADDRESS: &str = "0xC0FFEE";

const SPEC: &str = r#"
accounts = 10
txns = 40
seed = 1

[[init]]
function = "0xC0FFEE::counter::init"

[[calls]]
function = "0xC0FFEE::counter::increment"
"#;

fn compiled_counter() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/counter/build/counter/bytecode_modules/counter.mv")
}

#[test]
#[ignore = "requires the aptos build and a compiled counter fixture"]
fn shared_counter_is_the_only_hotspot() {
    let code = std::fs::read(compiled_counter())
        .expect("compile fixtures/counter with counter=0xC0FFEE first");
    let module_address = AccountAddress::from_hex_literal(COUNTER_ADDRESS).unwrap();
    let spec: WorkloadSpec = toml::from_str(SPEC).unwrap();

    let mut sandbox = Sandbox::new();
    let module_account = sandbox.account_at(module_address);
    let users = sandbox.create_accounts(spec.accounts);
    sandbox.publish_module(code).unwrap();

    for txn in plan_init(&spec, &module_account, &users).unwrap() {
        sandbox.run(txn).unwrap();
    }

    let mut written = HashSet::new();
    for planned in plan_calls(&spec, &users).unwrap() {
        let access = sandbox.run_and_capture(planned.txn).unwrap();
        assert!(access.success, "increment transaction failed");
        written.extend(access.writes);
    }

    let locations = sandbox.locations();
    let counters: Vec<_> = written
        .iter()
        .filter(|id| locations.label(**id).contains("Counter"))
        .collect();

    assert_eq!(
        counters.len(),
        1,
        "every increment must write the one shared Counter location"
    );
    assert_eq!(locations.class(*counters[0]), KeyClass::Resource);
}
