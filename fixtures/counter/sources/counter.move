/// A deliberately contended module: every `increment` reads and writes the one
/// global `Counter` at `@counter`, so the whole workload serializes on a single
/// state location. It is the validation oracle for the capture and model layers.
module counter::counter {
    struct Counter has key {
        value: u64,
    }

    public entry fun init(account: &signer) {
        move_to(account, Counter { value: 0 });
    }

    public entry fun increment(_caller: &signer) acquires Counter {
        let counter = borrow_global_mut<Counter>(@counter);
        counter.value = counter.value + 1;
    }
}
