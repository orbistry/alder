//! Reproducible full-solver measurements, excluding parse/canonicalization.
//! Run with `cargo bench -p alder-solve --bench inference`.

use std::alloc::{GlobalAlloc, Layout, System};
use std::fmt::Write;
use std::hint::black_box;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Instant;

use alder_ast::{ModuleId, PackageId};
use alder_can::Context;
use bumpalo::Bump;

struct CountingAllocator;

static MEASURING: AtomicBool = AtomicBool::new(false);
static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);
static BYTES: AtomicUsize = AtomicUsize::new(0);

fn count(size: usize) {
    if MEASURING.load(Ordering::Relaxed) {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        BYTES.fetch_add(size, Ordering::Relaxed);
    }
}

// SAFETY: every allocation and deallocation is forwarded unchanged to System.
// Counters do not allocate and the benchmark runs inference on one thread.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        count(layout.size());
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        count(layout.size());
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        count(size);
        unsafe { System.realloc(pointer, layout, size) }
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        unsafe { System.dealloc(pointer, layout) }
    }
}

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

fn main() {
    let samples = std::env::var("ALDER_BENCH_SAMPLES")
        .map(|value| value.parse::<usize>().expect("positive sample count"))
        .unwrap_or(15);
    assert!(samples > 0);
    println!("case,median_us,allocations,allocated_bytes,samples");
    for (name, source) in cases() {
        let mut timings = Vec::with_capacity(samples);
        let mut allocations = Vec::with_capacity(samples);
        let mut bytes = Vec::with_capacity(samples);
        for sample in 0..samples + 2 {
            let bump = Bump::new();
            let parsed = alder_parse::parse_module(&bump, bump.alloc_str(&source))
                .unwrap_or_else(|error| panic!("{name}: parse: {error:?}"));
            let canonical = alder_can::canonicalize(
                &bump,
                Context {
                    home: ModuleId {
                        package: PackageId::Application,
                        path: &["bench"],
                    },
                    imports: alder_can::resolve_imports(&bump, &parsed, PackageId::Application),
                    interfaces: &[],
                },
                &parsed,
            )
            .unwrap_or_else(|errors| panic!("{name}: canonicalize: {errors:?}"));
            let constraints = alder_constrain::constrain(&bump, canonical.module);
            let database = alder_solve::TraitDatabase::build(&bump, canonical.module, &[]);
            ALLOCATIONS.store(0, Ordering::Relaxed);
            BYTES.store(0, Ordering::Relaxed);
            MEASURING.store(true, Ordering::Relaxed);
            let started = Instant::now();
            let result = black_box(alder_solve::solve(&bump, &constraints, &database));
            let elapsed = started.elapsed();
            MEASURING.store(false, Ordering::Relaxed);
            assert!(result.is_ok(), "{name}: solve: {result:?}");
            if sample >= 2 {
                timings.push(elapsed.as_nanos());
                allocations.push(ALLOCATIONS.load(Ordering::Relaxed));
                bytes.push(BYTES.load(Ordering::Relaxed));
            }
        }
        timings.sort_unstable();
        allocations.sort_unstable();
        bytes.sort_unstable();
        println!(
            "{name},{:.3},{},{},{samples}",
            timings[samples / 2] as f64 / 1000.0,
            allocations[samples / 2],
            bytes[samples / 2],
        );
    }
}

fn cases() -> Vec<(&'static str, String)> {
    let mut chain = String::new();
    for index in 0..200 {
        writeln!(
            chain,
            "fn relay{index}(value) {{ relay{}(value) }}",
            index + 1
        )
        .unwrap();
    }
    chain.push_str("fn relay200(value) { value }\npub fn answer() { relay0(42) }\n");

    let mut shared = String::from(
        "fn duplicate(value) { (value, value) }\npub fn shared(value) {\nlet v0 = value\n",
    );
    for index in 1..=9 {
        writeln!(shared, "let v{index} = duplicate(v{})", index - 1).unwrap();
    }
    shared.push_str("v9\n}\n");

    let mut polymorphism = String::from("fn identity(value) { value }\n");
    for index in 0..100 {
        writeln!(
            polymorphism,
            "pub fn use{index}() {{ (identity({index}), identity(\"text\"), identity([true])) }}"
        )
        .unwrap();
    }

    let mut records = String::from("fn overlay(value, next) { { ..value, next } }\n");
    for index in 0..80 {
        writeln!(records, "pub fn record{index}() {{ overlay({{ count: {index}, name: \"name\", enabled: true }}, Some({index})) }}").unwrap();
    }

    let mut traits = String::from(
        "trait Render[a] { fn render(value: a) String }\nimpl Render[Number] { fn render(value) { show(value) } }\nimpl Render[Array[a]] where a: Render { fn render(values) { show(array.length(array.map(values, render))) } }\n",
    );
    for index in 0..80 {
        writeln!(
            traits,
            "pub fn render{index}() {{ render([[{index}, {index}]]) }}"
        )
        .unwrap();
    }

    vec![
        ("variable_chain", chain),
        ("shared_types", shared),
        ("polymorphism", polymorphism),
        ("records", records),
        ("traits", traits),
        ("pipes_fixture", fixture("pipes/src/main.ald")),
        (
            "async_traits_fixture",
            fixture("control_flow/src/methods.ald"),
        ),
        (
            "result_instances_fixture",
            fixture("traits/src/result_instances.ald"),
        ),
    ]
}

fn fixture(path: &str) -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/e2e")
        .join(path);
    std::fs::read_to_string(path).expect("run inference benchmarks from an Alder source checkout")
}
