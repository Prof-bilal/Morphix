//! Deterministic TOML fuzz harness for the untrusted manifest boundary (P2-15).
//!
//! The seed corpus lives in `tests/fixtures/fuzz/`. Mutations are generated with a local
//! xorshift engine so a failure is reproducible from the printed iteration and seed. The
//! harness only calls [`parse_manifest`], the one function allowed to see raw pack text:
//! any panic, hang, or non-SHA-256 hash is a failure.
//!
//! The normal test runs a bounded number of mutations with every `cargo test`. The ignored
//! test runs for 60 seconds and is invoked by CI (`cargo test ... -- --ignored`).

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use morphix_core::packs::parser::{MAX_MANIFEST_BYTES, parse_manifest};

const BOUNDED_ITERATIONS: usize = 20_000;
const FUZZ_SECONDS: u64 = 60;

const TOKENS: &[&[u8]] = &[
    b"\0",
    b"\xff",
    b"..",
    b"\"",
    b"[",
    b"]",
    b"{{",
    b"schema_version",
    b"\n[[previews]]\n",
    b"\xef\xbb\xbf",
];

struct Rng(u64);

impl Rng {
    fn next_u64(&mut self) -> u64 {
        // xorshift64: tiny, deterministic, and good enough for byte-level mutations.
        let mut value = self.0;
        value ^= value << 13;
        value ^= value >> 7;
        value ^= value << 17;
        self.0 = value;
        value
    }

    fn below(&mut self, bound: usize) -> usize {
        if bound == 0 {
            0
        } else {
            usize::try_from(self.next_u64() % u64::try_from(bound).unwrap_or(u64::MAX)).unwrap_or(0)
        }
    }
}

fn seed_directory() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("fuzz")
}

fn load_seeds() -> Vec<(PathBuf, Vec<u8>)> {
    let directory = seed_directory();
    let mut paths: Vec<PathBuf> = std::fs::read_dir(&directory)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", directory.display()))
        .filter_map(std::result::Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "toml")
        })
        .collect();
    paths.sort();
    assert!(
        paths.len() >= 4,
        "the fuzz seed corpus must contain at least four .toml files"
    );

    paths
        .into_iter()
        .map(|path| {
            let bytes = std::fs::read(&path)
                .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()));
            (path, bytes)
        })
        .collect()
}

fn mutate(data: &mut Vec<u8>, rng: &mut Rng) {
    let operation = rng.below(8);
    match operation {
        0 if !data.is_empty() => {
            let index = rng.below(data.len());
            data[index] ^= 1 << rng.below(8);
        }
        1 if !data.is_empty() => {
            let length = rng.below(data.len() + 1);
            data.truncate(length);
        }
        2 => {
            let token = TOKENS[rng.below(TOKENS.len())];
            let index = rng.below(data.len() + 1);
            data.splice(index..index, token.iter().copied());
        }
        3 if !data.is_empty() => {
            let start = rng.below(data.len());
            let end = start + rng.below(data.len() - start + 1);
            data.drain(start..end);
        }
        4 if !data.is_empty() => {
            let start = rng.below(data.len());
            let end = start + rng.below(data.len() - start + 1);
            let slice = data[start..end].to_vec();
            let index = rng.below(data.len() + 1);
            data.splice(index..index, slice);
        }
        5 if !data.is_empty() => {
            let index = rng.below(data.len());
            let token = TOKENS[rng.below(TOKENS.len())];
            data[index] = token[0];
        }
        6 if !data.is_empty() => {
            let index = rng.below(data.len());
            data[index] = u8::try_from(rng.below(256)).unwrap_or(0);
        }
        _ => {
            let token = TOKENS[rng.below(TOKENS.len())];
            let index = rng.below(data.len() + 1);
            data.splice(index..index, token.iter().copied());
        }
    }

    // The parser has its own size cap. Keeping the corpus near it exercises both the
    // early refusal and the parser proper without letting a seed grow without bound.
    if data.len() > usize::try_from(MAX_MANIFEST_BYTES).unwrap_or(usize::MAX) + 1024 {
        data.truncate(usize::try_from(MAX_MANIFEST_BYTES).unwrap_or(usize::MAX) + 1024);
    }
}

fn run_one(seeds: &[(PathBuf, Vec<u8>)], rng: &mut Rng) -> bool {
    let (_, seed) = &seeds[rng.below(seeds.len())];
    let mut data = seed.clone();
    let mutations = 1 + rng.below(6);
    for _ in 0..mutations {
        mutate(&mut data, rng);
    }

    match parse_manifest(&data) {
        Ok(parsed) => {
            assert!(
                parsed.manifest_hash.len() == 64
                    && parsed
                        .manifest_hash
                        .bytes()
                        .all(|byte| byte.is_ascii_hexdigit()),
                "a successful parse produced a malformed canonical hash"
            );
            assert_eq!(parsed.manifest.schema_version, 1);
            assert_eq!(
                parsed.manifest_bytes,
                u64::try_from(data.len()).unwrap_or(u64::MAX)
            );
            true
        }
        Err(_) => false,
    }
}

fn run_bounded(seeds: &[(PathBuf, Vec<u8>)], iterations: usize, rng: &mut Rng) {
    let mut accepted = 0usize;
    for _ in 0..iterations {
        if run_one(seeds, rng) {
            accepted += 1;
        }
    }
    assert!(
        accepted > 0,
        "no mutation parsed successfully; the harness is not exercising the success path"
    );
}

#[test]
fn mutated_manifests_never_panic_and_keep_a_canonical_hash() {
    let seeds = load_seeds();
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    run_bounded(&seeds, BOUNDED_ITERATIONS, &mut rng);
}

#[test]
#[ignore = "long-running: cargo test -p morphix-core --test parser_fuzz -- --ignored"]
fn fuzz_manifest_for_sixty_seconds() {
    let seeds = load_seeds();
    let deadline = Instant::now() + Duration::from_secs(FUZZ_SECONDS);
    let mut rng = Rng(0x2545_f491_4f6c_dd1d);
    let mut iterations = 0usize;
    let mut accepted = 0usize;
    while Instant::now() < deadline {
        if run_one(&seeds, &mut rng) {
            accepted += 1;
        }
        iterations += 1;
    }
    eprintln!("parser fuzz: {iterations} mutations, {accepted} accepted in {FUZZ_SECONDS}s");
    assert!(iterations > 0);
}
