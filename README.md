# Movespan

**A Block-STM contention profiler for Aptos Move.**

Aptos executes a block of transactions optimistically in parallel, then
re-executes only the ones that actually conflicted. That is Block-STM, and it is
the reason the chain is fast.

The catch is that a contract can quietly opt out of it. One counter that every
transaction increments, one pool object every swap mutates, and the whole block
collapses into a serial chain. The chain still does its job; your contract just
never uses it. Throughput is bad, and nothing in the toolchain tells you which
line of code is responsible.

Movespan tells you. It runs your contract under a workload in a real Aptos VM,
records what every transaction actually read and wrote, rebuilds the conflict
graph Block-STM would hit, and names the resource that is costing you
parallelism — then simulates the fix so you know what it is worth before you
refactor anything.

Think of it as a gas profiler, except it measures the one thing that only
matters on a parallel chain.

## What it looks like

```
$ movespan analyze --package fixtures/amm --module-address 0xA11CE \
      --workload workloads/amm.toml --threads 32

MOVESPAN CONTENTION REPORT
==========================

Transactions       : 200
Threads modeled    : 32
Parallelizability  : 3.1%
Estimated speedup  : 1.00x of 32x ideal
Work / span        : 82412 / 82412
Conflict edges     : 235

TOP CONTENTION HOTSPOTS
-----------------------
 1. 0xa11ce::amm::Pool  (199 deps, blocked cost 82372)
 2. 0x700d...::amm::LpBalance  (5 deps, blocked cost 250)

RECOMMENDATIONS
---------------

[1] 0xa11ce::amm::Pool
    problem: 0xa11ce::amm::Pool is a shared resource many transactions mutate together.
    fix    : Split independent fields into separate resources, or shard the resource
             per user address so unrelated writers stop colliding on one object.
    impact : 1.00x -> 9.11x  (+8.11x, 28% parallel)
```

The last line is the point. Not "your contract is slow" — *shard this one
resource and the workload goes from serial to 9x*.

## How it works

```
Move package ──▶ Aptos VM ──▶ per-txn read/write sets ──▶ conflict DAG ──▶ hotspots ──▶ simulated fixes
                 (real execution)   (RecordingStateView)      (RAW edges)     (ranked)    (re-run model)
```

Four stages:

**1. Execute.** The package is published into an in-memory chain and the
workload runs against it. These are real transactions through the real Aptos
VM — not a simulation of one.

**2. Capture.** `RecordingStateView` wraps the executor's state view and
observes every read; writes come from each transaction's `WriteSet`. Access sets
are therefore *observed*, not inferred by static analysis. This is the design
decision the accuracy of everything else rests on: static analysis cannot know
which dynamic address a transaction will touch, and a wrong access set produces
a confidently wrong report.

Each transaction runs as its own single-transaction block. Block execution is
parallel by nature and would interleave reads across transactions, making
per-transaction attribution impossible.

**3. Model.** Build the read-after-write dependency graph: transaction `j`
depends on `i` when `j` reads a location `i` wrote. Then estimate the parallel
makespan with a greedy list scheduler over Block-STM's worker threads, using
each transaction's gas as its cost. `work / span` is the classic critical-path
pair — total work over the longest dependency chain — and their ratio is the
parallelism actually available.

**4. Advise.** Rank locations by how much work they block, map each to a
concrete Aptos-specific refactor, then *re-run the entire model* with that
location made non-conflicting. The projected speedup is measured against the
model, not guessed at.

### What is deliberately excluded

Three classes of state are not counted as contention, because counting them
produces misleading reports:

| Excluded | Why |
| --- | --- |
| Aggregators / delayed fields | Their updates commute; Block-STM does not serialize on them |
| Module code | Written only at publish time, never during a workload |
| Aptos framework state | Gas payment, sequence numbers, fungible stores |

The framework case matters more than it sounds. Every transaction pays gas and
bumps a sequence number, so those locations touch 100% of a block and will
out-rank whatever your contract actually does wrong. Worse, they are
load-bearing for the VM and cannot be refactored by a contract author — so a
report that ranks them tells you something true and completely useless.
Framework state is identified by the *defining* address of the struct tag (a
reserved address such as `0x1` or `0xa`), not by where the value happens to be
stored.

## Architecture

Two sibling Cargo workspaces, split on purpose:

| Path | Workspace | Aptos deps | Responsibility |
| --- | --- | --- | --- |
| `crates/movespan-types` | root | no | Interned locations, captured access sets |
| `crates/movespan-model` | root | no | Dependency DAG, scheduler, metrics |
| `crates/movespan-rules` | root | no | Hotspot → suggestion, simulate-the-fix |
| `crates/movespan-report` | root | no | text / json / html rendering |
| `app/movespan-core` | `app/` | yes | VM capture, sandbox (Mode B), replay (Mode A) |
| `app/movespan-cli` | `app/` | yes | the `movespan` binary |

The split is the most important structural decision in the repo. Everything
conceptually interesting — the conflict model, the scheduler, the rules engine —
lives in the root workspace and has **no Aptos dependency at all**. It compiles
and tests in under a second.

Only the capture layer needs aptos-core, which is a multi-gigabyte git
dependency and a heavy one-time build. Keeping it in a separate workspace means
you can read, modify, and test the core of this tool without ever building
Aptos. It also means the model is unit-testable against synthetic access sets,
which is how the correctness claim below is verified.

Data flows in one direction: `types` ← `model` ← `rules` ← `report`, with
`core` producing types and `cli` consuming reports. No cycles, no shared mutable
state.

## Input modes

**Mode B — sandbox** (`analyze`, `footprint`). Publish a compiled package into a
fresh in-memory chain and run a synthetic weighted workload against it. This
manufactures realistic contention — many users, one pool — with **zero real
users**, which makes it usable on a contract that has never been deployed.

**Mode A — replay** (`replay`). Fork live network state at a version and
re-execute historical transactions, for numbers from real traffic. Requires a
fullnode that serves historical state at that version.

## Quickstart

**1. Prove the model** — fast, no Aptos build required:

```sh
cargo test --workspace
```

**2. Build the profiler** — one-time heavy aptos-core build:

```sh
cd app && cargo build --release --locked
```

**3. Profile a contract:**

```sh
# Compile a fixture to a fixed address
(cd fixtures/counter && aptos move compile --named-addresses counter=0xC0FFEE)

# Analyze it
app/target/release/movespan analyze \
  --package fixtures/counter \
  --module-address 0xC0FFEE \
  --workload workloads/counter.toml
```

### Building against aptos-core

Building out-of-tree against aptos-core means replicating its build harness. All
five requirements are committed, so the build above should just work — but if
you are adapting this, these are the constraints, and each one was found by a
failed build:

| Requirement | Where it lives |
| --- | --- |
| Aptos pinned to tag `aptos-node-v1.48.2` (never `main`) | `app/Cargo.toml` |
| Toolchain `1.94.1` — newer rustc rejects some aptos crates | `app/rust-toolchain.toml` |
| `--cfg tokio_unstable` — aptos uses unstable tokio APIs | `app/.cargo/config.toml` |
| Vendored lockfile, built with `--locked` | `app/Cargo.lock` |
| `[patch.crates-io]` mirroring aptos's forks of `merlin`, `futures`, `ark-*` | `app/Cargo.toml` |

Build from **inside** `app/`, since cargo discovers the toolchain and config
from the working directory rather than from `--manifest-path`.

The lockfile is not optional. A fresh resolution pulls newer final releases of
the crypto crates (`pkcs8`, `der`, `spki`) that the aptos crypto stack does not
compile against; `--locked` pins the exact set Aptos ships.

## Measured results

Both fixtures, captured through the real VM and analysed end to end:

| Fixture | Workload | Parallelizability | Top hotspot | Simulated fix |
| --- | --- | --- | --- | --- |
| `counter` | 100 txns, 8 threads | 12.5% | `counter::Counter` (99 deps) | **1.00x → 7.69x** |
| `amm` | 200 txns, 32 threads | 3.1% | `amm::Pool` (199 deps) | **1.00x → 9.11x** |

**The counter is a correctness oracle, not a benchmark.** 100 transactions
against one global counter *must* produce exactly 99 read-after-write
dependencies, a fully serial schedule, and a score of exactly `1/8` on 8
threads. It produces 99, serial, and 12.5%. That is checkable by hand in a
couple of minutes, and it is why the model is trustworthy rather than merely
plausible.

The AMM is the realistic case: a shared `Pool` serializes every swap while
per-user `LpBalance` writes stay independent, so sharding the pool is worth
+8.11x.

These numbers come from a debug build, hence the reduced transaction counts. The
committed workloads use 2000 transactions and want a `--release` build.

## Accuracy and limits

Being precise about what these numbers are:

- **Exact:** the read/write sets. They come from the VM, not from analysis.
- **Exact:** the dependency graph, hotspot ranking, and the direction and
  relative magnitude of a fix.
- **An estimate:** absolute throughput. The scheduler models Block-STM's
  behaviour but is not a validator. Treat `9.11x` as "this is a large win worth
  doing", not as a TPS guarantee.
- **Not modeled:** validator-level effects — network, consensus, storage I/O,
  and Block-STM's own re-execution and abort overhead.

Mode A (mainnet replay) is implemented but has not yet been exercised against a
live fullnode.

## CLI

```
movespan analyze   --package <dir> --module-address <0x..> --workload <toml>
movespan footprint --package <dir> --module-address <0x..> --workload <toml>
movespan replay    --network <mainnet|testnet|devnet|url> --start-version <v> --count <n>

  --threads <n>      Block-STM workers to model (default 32)
  --format <text|json|html>
  --out <file>       write the report instead of printing it
  --min-score <f>    exit non-zero if parallelizability is below f (CI guard)
```

`footprint` skips the model and dumps the raw storage access sets — useful when
you want to see what a workload actually touches, or to debug a surprising
result.

## Workload spec

A workload describes who sends what. Constants force shared access and create
contention; random values spread it out.

```toml
accounts = 50      # funded senders
txns = 2000        # transactions to generate
seed = 42          # deterministic — same seed, same workload

[[init]]                                   # run once, by the package account
function = "0xA11CE::amm::create_pool"
args = [
  { kind = "const_u64", value = 1000000 },
  { kind = "const_u64", value = 1000000 },
]

[[calls]]                                  # measured, drawn by weight
function = "0xA11CE::amm::swap"
weight = 70
args = [{ kind = "rand_u64", min = 1, max = 1000 }]

[[calls]]
function = "0xA11CE::amm::add_liquidity"
weight = 30
args = [{ kind = "rand_u64", min = 1, max = 500 }]
```

Argument generators: `const_u64`, `rand_u64 { min, max }`, `const_address`,
`rand_account`.

Workloads are seeded, so a run is reproducible — which is what makes the CI
guard below meaningful.

## CI adoption

`.github/workflows/movespan-guard.yml` is a reusable workflow. Point it at a
package and a workload and it fails any pull request that drops
parallelizability below a threshold — catching contention regressions the way a
coverage gate catches untested code.

```yaml
- run: movespan analyze --package . --module-address 0xA11CE \
         --workload workloads/amm.toml --min-score 0.4
```

## License

MIT — see [LICENSE](LICENSE).
