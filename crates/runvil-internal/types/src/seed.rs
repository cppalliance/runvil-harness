//! A run's seed and the values derived from it.
//!
//! The caller draws a run's [`RunSeed`] from a CSPRNG for a live run and
//! passes the recorded one back for a replay. Every value derived from the
//! seed goes through `derive()` under its own `runvil.<purpose>.v1`
//! tag, as the guard nonce
//! ([`GuardNonce::from_seed`](crate::untrusted::GuardNonce::from_seed))
//! and the source of the `math.random` seeds ([`LuaRandom`]) do. Each
//! derivation is one-way, so a derived value can be shown without revealing
//! the seed or any other value derived from it.

use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use sha2::{Digest as _, Sha256};

/// What a run seed's text form must be, for a refusal to name.
const REQUIRED: &str = "a run seed is 64 lowercase hex characters";

/// Holds the 32 bytes the caller supplies to seed a run.
///
/// Every value derived for the run comes from the seed. `Display` and serde
/// use 64 lowercase hex characters. `Debug` prints `RunSeed(..)` with no
/// bytes, so a trace, panic, or test failure never shows the seed, from
/// which anyone could compute the run's nonce.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct RunSeed([u8; 32]);

impl RunSeed {
    /// Returns the seed whose bytes are `bytes`.
    #[must_use]
    pub const fn from_bytes(bytes: [u8; 32]) -> RunSeed {
        RunSeed(bytes)
    }

    /// Returns the seed's 32 bytes.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl fmt::Display for RunSeed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write_hex(f, &self.0)
    }
}

impl fmt::Debug for RunSeed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("RunSeed(..)")
    }
}

impl Serialize for RunSeed {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for RunSeed {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        parse_hex(&text)
            .map(RunSeed)
            .map_err(serde::de::Error::custom)
    }
}

/// A run's source of the seed of each section VM's `math.random` generator.
///
/// Stock Lua seeds the generator from the clock and a memory address, so a
/// draw would differ between two runs with the same inputs. The Engine
/// seeds each section VM from this value and the section entry's id
/// instead: the draws differ from one run to the next and from one section
/// entry to the next, and a run with the same seed repeats them.
///
/// The value is the digest of the run's seed under its own tag. Lua's
/// generator is not cryptographic, so assume that a prompt which reads
/// enough draws can compute the 128 bits that seeded its VM. Those bits
/// are a digest of this value, which is itself a digest of the seed, so
/// they give back neither the seed, nor the guard nonce, nor another
/// entry's seed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LuaRandom([u8; 32]);

impl LuaRandom {
    /// Derives the run's source from `seed`.
    #[must_use]
    pub fn from_seed(seed: &RunSeed) -> LuaRandom {
        LuaRandom(derive(LUA_RANDOM_DOMAIN, seed))
    }

    /// The seed of the generator of the section entry `entry_id`: the two
    /// integers `math.randomseed` takes.
    ///
    /// They are the first 16 bytes of SHA-256 over this source, then
    /// `entry_id`'s bytes, read as two big-endian words. No two section
    /// entries of a run share an id, so no two of its VMs share a seed.
    /// The derivation is part of the run's replay contract and never
    /// changes silently.
    #[must_use]
    pub fn entry_seed(&self, entry_id: &str) -> [i64; 2] {
        let digest = Sha256::new()
            .chain_update(self.0)
            .chain_update(entry_id)
            .finalize();
        let (words, _) = digest.as_chunks::<8>();
        [words[0], words[1]].map(i64::from_be_bytes)
    }
}

/// SHA-256 over `domain`, then `seed`'s bytes.
///
/// Every value derived from a run's seed comes from here under its own
/// `runvil.<purpose>.v1` tag, so no two purposes share a digest. The
/// derivation is part of the run's replay contract and never changes
/// silently.
#[must_use]
pub(crate) fn derive(domain: &[u8], seed: &RunSeed) -> [u8; 32] {
    let digest = Sha256::new()
        .chain_update(domain)
        .chain_update(seed.0)
        .finalize();
    let mut out = [0u8; 32];
    out.copy_from_slice(&digest);
    out
}

/// The domain tag hashed ahead of the seed, so the source of the
/// `math.random` seeds never equals a SHA-256 of the same seed bytes taken
/// for any other purpose.
const LUA_RANDOM_DOMAIN: &[u8] = b"runvil.lua-random.v1";

/// Writes `bytes` as lowercase hex.
fn write_hex(f: &mut fmt::Formatter<'_>, bytes: &[u8; 32]) -> fmt::Result {
    bytes.iter().try_for_each(|byte| write!(f, "{byte:02x}"))
}

/// The 32 bytes `text` spells in 64 lowercase hex characters, or a refusal
/// naming the first character outside that form or the actual length.
fn parse_hex(text: &str) -> Result<[u8; 32], String> {
    if let Some((at, found)) = text
        .char_indices()
        .find(|(_, c)| !matches!(c, '0'..='9' | 'a'..='f'))
    {
        return Err(format!("{REQUIRED}, found {found:?} at byte {at}"));
    }
    if text.len() != 64 {
        return Err(format!("{REQUIRED}, found {} characters", text.len()));
    }
    let mut bytes = [0u8; 32];
    let (pairs, _) = text.as_bytes().as_chunks::<2>();
    for (byte, [high, low]) in bytes.iter_mut().zip(pairs) {
        *byte = (nibble(*high) << 4) | nibble(*low);
    }
    Ok(bytes)
}

/// The value of one lowercase hex digit `parse_hex` has already checked.
const fn nibble(digit: u8) -> u8 {
    match digit {
        b'0'..=b'9' => digit - b'0',
        _ => digit - b'a' + 10,
    }
}

#[cfg(test)]
mod tests {
    //! Tests for the run seed and the values derived from it: the derivation's
    //! known answer, the nonce it yields, the seed's serde form, and its
    //! redacted `Debug`.

    use std::fmt::Write as _;

    use super::*;
    use crate::test_support::{SEED, Seeded, assert_wire};
    use crate::untrusted::GuardNonce;

    /// `bytes` as lowercase hex.
    fn hex(bytes: &[u8]) -> String {
        bytes.iter().fold(String::new(), |mut out, byte| {
            write!(out, "{byte:02x}").expect("a String takes every write");
            out
        })
    }

    #[test]
    fn derive_matches_its_known_answer() {
        // SHA-256 over `runvil.guard-nonce.v1` then the bytes 0x00..=0x1f,
        // computed once outside this crate.
        let seed = RunSeed::from_bytes(std::array::from_fn(|i| {
            u8::try_from(i).expect("an index below 32 fits a byte")
        }));
        let expected = "0d89f97db656f4554ed91f0dc4bc9a829d2555c04535cceffdda52e6d44b66cf";
        assert_eq!(hex(&derive(b"runvil.guard-nonce.v1", &seed)), expected);
        assert_eq!(
            GuardNonce::from_seed(&seed).to_string(),
            expected[..32],
            "the nonce is the first half of derive under its own tag"
        );
    }

    #[test]
    fn a_lua_random_entry_seed_matches_its_known_answer() {
        let seed = RunSeed::from_bytes(std::array::from_fn(|i| {
            u8::try_from(i).expect("an index below 32 fits a byte")
        }));
        let random = LuaRandom::from_seed(&seed);
        // Computed outside this crate, with another SHA-256, so a change to
        // the tag, the hashing order, or the byte order fails here.
        assert_eq!(
            random.entry_seed("0.0"),
            [-3_572_835_231_721_551_567, -8_962_037_305_301_457_989]
        );
        assert_eq!(
            random.entry_seed("0.1"),
            [-1_382_469_946_683_854_880, -7_771_455_195_306_047_233]
        );
    }

    #[test]
    fn the_nonce_differs_from_the_seed_and_repeats() {
        let mut rng = Seeded::new(SEED);
        let seed = rng.run_seed();
        let nonce = GuardNonce::from_seed(&seed).to_string();
        let bytes = hex(seed.as_bytes());
        rng.check(
            !bytes.starts_with(&nonce),
            format_args!("the nonce {nonce} is the seed's prefix"),
        );
        rng.check_eq(
            &GuardNonce::from_seed(&seed).to_string(),
            &nonce,
            "one seed, one nonce",
        );
    }

    #[test]
    fn a_run_seed_round_trips_through_serde_as_64_hex_characters() {
        let mut rng = Seeded::new(SEED);
        let seed = rng.run_seed();
        let rendered = hex(seed.as_bytes());
        rng.check_eq(&seed.to_string(), &rendered, "Display is the serde form");
        assert_wire(&seed, &format!("\"{rendered}\""));

        let short = serde_json::from_str::<RunSeed>("\"abc\"")
            .expect_err("three characters are not a seed")
            .to_string();
        assert!(
            short.contains("64 lowercase hex characters") && short.contains("found 3"),
            "the refusal names the required form and the actual length: {short}"
        );
        let upper = serde_json::from_str::<RunSeed>(&format!("\"A{}\"", &rendered[1..]))
            .expect_err("an uppercase digit is not a seed")
            .to_string();
        assert!(
            upper.contains("64 lowercase hex characters") && upper.contains("'A'"),
            "the refusal names the required form and the actual character: {upper}"
        );
    }

    #[test]
    fn a_run_seeds_debug_shows_no_seed_bytes() {
        let seed = RunSeed::from_bytes([0xab; 32]);
        assert_eq!(format!("{seed:?}"), "RunSeed(..)");
        assert_eq!(format!("{seed:#?}"), "RunSeed(..)");
    }
}
