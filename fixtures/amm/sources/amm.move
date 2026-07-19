/// A minimal constant-product AMM used as a realistic contention benchmark.
/// Every `swap` and `add_liquidity` reads and writes the single global `Pool`
/// at `@amm`, so throughput serializes on one location even though the per-user
/// `LpBalance` writes are independent. This is the pattern Movespan is built to
/// surface and quantify a fix for.
module amm::amm {
    use std::signer;

    const E_POOL_EXISTS: u64 = 1;

    struct Pool has key {
        reserve_x: u64,
        reserve_y: u64,
        total_lp: u64,
    }

    struct LpBalance has key {
        amount: u64,
    }

    public entry fun create_pool(account: &signer, reserve_x: u64, reserve_y: u64) {
        assert!(!exists<Pool>(signer::address_of(account)), E_POOL_EXISTS);
        move_to(account, Pool { reserve_x, reserve_y, total_lp: reserve_x });
    }

    public entry fun swap(_user: &signer, amount_in: u64) acquires Pool {
        let pool = borrow_global_mut<Pool>(@amm);
        let amount_out = (pool.reserve_y * amount_in) / (pool.reserve_x + amount_in);
        pool.reserve_x = pool.reserve_x + amount_in;
        if (amount_out < pool.reserve_y) {
            pool.reserve_y = pool.reserve_y - amount_out;
        };
    }

    public entry fun add_liquidity(user: &signer, amount: u64) acquires Pool, LpBalance {
        let pool = borrow_global_mut<Pool>(@amm);
        pool.reserve_x = pool.reserve_x + amount;
        pool.total_lp = pool.total_lp + amount;

        let addr = signer::address_of(user);
        if (exists<LpBalance>(addr)) {
            let balance = borrow_global_mut<LpBalance>(addr);
            balance.amount = balance.amount + amount;
        } else {
            move_to(user, LpBalance { amount });
        }
    }
}
