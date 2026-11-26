//! Parses the frontmatter contract keys: `uses`, `args`, `models`,
//! `prompts`, and `annotations`.
//!
//! Submodules own the argument schema, model requirements, and tool
//! annotations. This root checks Pack and prompt lists, validates aliases,
//! and reads contract maps. Parsing validates declarations' static shape;
//! the run's first step checks them against the caller's environment and
//! arguments. The Engine never reads the annotations during execution.

use std::collections::btree_map::Entry;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::marker::PhantomData;

use runvil_types::models::ModelRole;
use runvil_types::names::is_alias;
use runvil_types::packs::PackRef;
use serde::de::{self, MapAccess, Visitor};
use serde::{Deserialize, Deserializer};

use crate::error;

mod annotations;
mod args;
mod args_check;
mod models;

#[cfg(test)]
mod tests;

pub use annotations::Annotations;
pub use args::ArgsSchema;
pub(crate) use args::TEXT_FIELD;
pub(crate) use args_check::accept as accept_args;
pub(crate) use args_check::accept_value as accept_args_value;
pub(crate) use models::RoleMap;
pub use models::RoleRequirements;

/// A key of a contract map: how it is checked and how an error names it.
trait ContractKey: Ord {
    /// Refuses a key the map's grammar does not allow.
    fn check(&self, what: &str) -> Result<(), String>;

    /// The key as an error names it.
    fn shown(&self) -> String;
}

/// Refuses `name` unless it follows the alias grammar, naming it as a
/// `what`, such as `argument name`, in the refusal.
pub(crate) fn check_alias(name: &str, what: &str) -> Result<(), String> {
    if is_alias(name) {
        Ok(())
    } else {
        Err(format!(
            "invalid {what} `{name}`: expected [A-Za-z][A-Za-z0-9_-]{{0,63}}"
        ))
    }
}

impl ContractKey for String {
    fn check(&self, what: &str) -> Result<(), String> {
        check_alias(self, what)
    }

    fn shown(&self) -> String {
        self.clone()
    }
}

impl ContractKey for ModelRole {
    /// A role name is checked while it deserializes, against the five names.
    fn check(&self, _what: &str) -> Result<(), String> {
        Ok(())
    }

    fn shown(&self) -> String {
        self.as_str().to_owned()
    }
}

/// Deserializes a contract map (`models`, and each `properties` map under
/// `args`): keys of type `K` checked
/// by [`ContractKey`], values deserialized as `T`, duplicates rejected. `what`
/// names the keys in every error.
fn deserialize_contract_map<'de, D, K, T>(
    deserializer: D,
    what: &'static str,
) -> Result<BTreeMap<K, T>, D::Error>
where
    D: Deserializer<'de>,
    K: ContractKey + Deserialize<'de>,
    T: Deserialize<'de>,
{
    deserializer.deserialize_map(MapVisitor {
        what,
        _marker: PhantomData,
    })
}

/// The visitor behind [`deserialize_contract_map`]: a streaming map walk so
/// rejections keep their source position.
struct MapVisitor<K, T> {
    /// The key kind (`model role` or `argument name`), for error messages.
    what: &'static str,
    /// The key and value types, without ownership or variance claims.
    _marker: PhantomData<fn() -> (K, T)>,
}

impl<'de, K, T> Visitor<'de> for MapVisitor<K, T>
where
    K: ContractKey + Deserialize<'de>,
    T: Deserialize<'de>,
{
    type Value = BTreeMap<K, T>;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "a map of {} keys to declarations", self.what)
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let what = self.what;
        let mut entries: BTreeMap<K, T> = BTreeMap::new();
        while let Some(key) = map.next_key::<K>()? {
            key.check(what).map_err(de::Error::custom)?;
            match entries.entry(key) {
                Entry::Occupied(entry) => {
                    return Err(de::Error::custom(format!(
                        "duplicate {what} `{}`: contract map keys must be unique",
                        entry.key().shown()
                    )));
                }
                Entry::Vacant(entry) => {
                    entry.insert(map.next_value::<T>()?);
                }
            }
        }
        Ok(entries)
    }
}

/// Refuses a `uses:` list that names one Pack id twice, whatever the
/// vendors, because a run holds one Pack per Pack id.
///
/// The refusal names the first Pack id declared again, with the `Reason`
/// fact `pack_duplicate`.
pub(crate) fn check_distinct_packs(packs: &[PackRef]) -> crate::Result<()> {
    let mut seen = BTreeSet::new();
    match packs.iter().find(|pack| !seen.insert(pack.id())) {
        Some(pack) => Err(error::make_pack_refusal_error(
            "pack_duplicate",
            pack.id(),
            format!(
                "invalid frontmatter: Pack id {} is declared more than once under uses",
                pack.id()
            ),
        )),
        None => Ok(()),
    }
}

/// Refuses a `uses:` list that names a reserved Pack id, such as `adhoc`,
/// whatever the vendor, because the Engine holds the tools under it.
///
/// The refusal names the first reserved Pack id declared, with the
/// `Reason` fact `pack_reserved`.
pub(crate) fn check_no_reserved_pack(packs: &[PackRef]) -> crate::Result<()> {
    match packs.iter().find(|pack| pack.id().is_reserved()) {
        Some(pack) => Err(error::make_pack_refusal_error(
            "pack_reserved",
            pack.id(),
            format!(
                "invalid frontmatter: Pack id {} is reserved by the Engine and is never declared \
                 under uses",
                pack.id()
            ),
        )),
        None => Ok(()),
    }
}

/// Refuses a `prompts:` list that holds an empty entry or names one prompt
/// twice, because each entry is the one name a finder finds a prompt
/// under.
///
/// The refusal is for the first such entry, with the `Reason` fact
/// `prompt_empty` or `prompt_duplicate`, and a repeated name is its `Name`
/// fact.
pub(crate) fn check_listed_prompts(names: &[String]) -> crate::Result<()> {
    let mut seen = BTreeSet::new();
    for name in names {
        if name.is_empty() {
            return Err(error::make_prompts_refusal_error(
                "prompt_empty",
                None,
                "invalid frontmatter: a prompts entry must not be empty".to_owned(),
            ));
        }
        if !seen.insert(name) {
            return Err(error::make_prompts_refusal_error(
                "prompt_duplicate",
                Some(name),
                format!(
                    "invalid frontmatter: prompt {name} is listed more than once under prompts"
                ),
            ));
        }
    }
    Ok(())
}
