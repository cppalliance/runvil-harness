//! The shared ordering of a Lua table's hash keys.
//!
//! Lua leaves a table's hash traversal order unspecified, so every path that
//! turns a table into an ordered sequence imposes an order of its keys:
//! booleans first (`false` before `true`), then numbers by value, then
//! strings bytewise. [`sort_key`] classifies one Lua key into its
//! [`SortKey`]; [`SortKey::compare`] orders two of them, and `json.encode`
//! visits the keys of a table that way. A walk of a table goes by
//! [`WalkPlace`], which puts the integer keys from 1 up ahead of the rest:
//! the deterministic `pairs`/`next` installer and fanout's member
//! enumeration share it, so a walk of a table is a function of its
//! contents wherever it is made. [`FirstRefusal`] gives the order of
//! [`SortKey`] to a check that refuses a table for one of its entries,
//! and [`sort_list`] puts a Lua list of a table's entries in the order
//! its caller gives their places. [`listed`] counts the items that come
//! before the keys.

use std::cmp::Ordering;

use mlua::{Table, Value};

/// A hash key's sort position: booleans first (`false` before `true`), then
/// numbers by value, then strings bytewise. The ranks keep mixed-type keys
/// totally ordered without inventing a cross-type comparison. An integer
/// key stays an `i64` so two distinct integers past 2^53 never compare
/// equal (which would leave their order to `pairs`, the nondeterminism the
/// sort exists to remove); only a mixed integer/float pair converts.
#[derive(Debug)]
pub(crate) enum SortKey {
    Bool(bool),
    Integer(i64),
    Float(f64),
    Text(Vec<u8>),
}

impl SortKey {
    /// The type's ordering rank: booleans, then numbers, then strings.
    fn rank(&self) -> u8 {
        match self {
            SortKey::Bool(_) => 0,
            SortKey::Integer(_) | SortKey::Float(_) => 1,
            SortKey::Text(_) => 2,
        }
    }

    /// Orders two positions of any type: same types compare within their
    /// type, a mixed integer and float compares exactly, and a mixed type
    /// falls back to its rank.
    pub(crate) fn compare(&self, other: &SortKey) -> Ordering {
        match (self, other) {
            (SortKey::Bool(left), SortKey::Bool(right)) => left.cmp(right),
            (SortKey::Integer(left), SortKey::Integer(right)) => left.cmp(right),
            (SortKey::Float(left), SortKey::Float(right)) => left.total_cmp(right),
            (SortKey::Integer(integer), SortKey::Float(float)) => {
                compare_integer_float(*integer, *float)
            }
            (SortKey::Float(float), SortKey::Integer(integer)) => {
                compare_integer_float(*integer, *float).reverse()
            }
            (SortKey::Text(left), SortKey::Text(right)) => left.cmp(right),
            _ => self.rank().cmp(&other.rank()),
        }
    }
}

/// Orders an integer key against a float key exactly: the float is
/// compared to the integer's neighborhood without rounding the integer,
/// so an integer past 2^53 still sorts on the correct side of a nearby
/// float. A float outside `i64`'s range is beyond every integer; a float
/// inside it is truncated, the integer parts are compared, and a tie is
/// broken by the float's fractional part (an exact integer-valued float
/// ties with its integer).
fn compare_integer_float(integer: i64, float: f64) -> Ordering {
    /// 2^63: one past `i64::MAX`, exactly representable, so a float at or
    /// beyond it is greater than every integer.
    const ABOVE_MAX: f64 = 9_223_372_036_854_775_808.0;
    /// -2^63: exactly `i64::MIN`, so a float below it is less than every
    /// integer.
    const MIN: f64 = -9_223_372_036_854_775_808.0;
    if float >= ABOVE_MAX {
        return Ordering::Less;
    }
    if float < MIN {
        return Ordering::Greater;
    }
    // In range and finite: the truncation is exact for the integer part.
    #[expect(
        clippy::cast_possible_truncation,
        reason = "the float is inside i64's range and its fractional part is compared separately"
    )]
    let truncated = float.trunc() as i64;
    match integer.cmp(&truncated) {
        Ordering::Equal => {
            // The integer equals the float's integer part, so it sits below
            // a float with a positive fraction and above one with a
            // negative fraction.
            let fraction = float - float.trunc();
            if fraction > 0.0 {
                Ordering::Less
            } else if fraction < 0.0 {
                Ordering::Greater
            } else {
                Ordering::Equal
            }
        }
        ordering => ordering,
    }
}

/// Why a Lua value cannot be a sortable hash key.
#[derive(Debug)]
pub(crate) enum KeyError {
    /// A number key that is NaN or infinite.
    NotFinite,
    /// A string key whose bytes are not valid UTF-8, so it has no label.
    NotUtf8(mlua::Error),
    /// A key whose Lua type is not string, number, or boolean.
    Unsortable(&'static str),
}

/// Classifies a Lua value as a sortable hash key: its sort position plus the
/// label naming it (a string's text, any other scalar's rendering).
///
/// # Errors
/// Returns [`KeyError`] when the value is not a string, number, or boolean,
/// when a string's bytes are not valid UTF-8 (it has no label), or when a
/// number is not finite. Each caller maps the failure onto its own
/// diagnostic, so this helper names no feature.
pub(crate) fn sort_key(key: &Value) -> Result<(SortKey, String), KeyError> {
    match key {
        Value::String(s) => {
            let label = s.to_str().map_err(KeyError::NotUtf8)?.to_owned();
            Ok((SortKey::Text(s.as_bytes().to_vec()), label))
        }
        Value::Integer(i) => Ok((SortKey::Integer(*i), i.to_string())),
        Value::Number(n) => {
            if n.is_finite() {
                Ok((SortKey::Float(*n), n.to_string()))
            } else {
                Err(KeyError::NotFinite)
            }
        }
        Value::Boolean(b) => Ok((SortKey::Bool(*b), b.to_string())),
        other => Err(KeyError::Unsortable(other.type_name())),
    }
}

/// A key's place in a walk of its table, for a caller that needs no label:
/// that of [`sort_key`], and also a string that is not valid UTF-8 by its
/// bytes and an infinite number by its value. Lua's own order of two such
/// keys comes from its hash state, so a walk that kept it would differ from
/// run to run.
///
/// # Errors
/// Returns the key's type name when the key is a table, a function, a
/// userdata, or a thread. Only its memory address tells two such keys
/// apart, and no order made from an address is the same in two runs.
pub(crate) fn position(key: &Value) -> Result<SortKey, &'static str> {
    match (sort_key(key), key) {
        (Ok((position, _)), _) => Ok(position),
        (Err(KeyError::NotUtf8(_)), Value::String(text)) => {
            Ok(SortKey::Text(text.as_bytes().to_vec()))
        }
        (Err(KeyError::NotFinite), Value::Number(number)) => Ok(SortKey::Float(*number)),
        (Err(_), other) => Err(other.type_name()),
    }
}

/// Orders the places [`position`] gives two keys: a key with a place comes
/// before a key without one, and two keys without one go by type name.
pub(crate) fn compare_places(
    left: &Result<SortKey, &'static str>,
    right: &Result<SortKey, &'static str>,
) -> Ordering {
    match (left, right) {
        (Ok(left), Ok(right)) => left.compare(right),
        (Ok(_), Err(_)) => Ordering::Less,
        (Err(_), Ok(_)) => Ordering::Greater,
        (Err(left), Err(right)) => left.cmp(right),
    }
}

/// Where a key sits in the walk order: an integer from 1 up comes before
/// every other key, by its value, and the other keys follow by
/// [`SortKey`]. So the items of a list precede a boolean key, though
/// [`SortKey`] ranks a boolean below a number.
///
/// `pairs`, `next`, and the members of a `tasks.fanout` collection go in
/// this order, so one table walks one way. The place is the key's own: it
/// does not follow where the list of its table ends.
pub(crate) enum WalkPlace {
    /// An integer key from 1 up.
    Index(i64),
    /// Any other key: never an integer from 1 up.
    Scalar(SortKey),
}

impl WalkPlace {
    /// The place of `key`, or the name of its type when the order has no
    /// place for it.
    pub(crate) fn of(key: &Value) -> Result<Self, &'static str> {
        position(key).map(Self::from)
    }

    /// Orders two places.
    pub(crate) fn compare(&self, other: &WalkPlace) -> Ordering {
        match (self, other) {
            (WalkPlace::Index(left), WalkPlace::Index(right)) => left.cmp(right),
            (WalkPlace::Index(_), WalkPlace::Scalar(_)) => Ordering::Less,
            (WalkPlace::Scalar(_), WalkPlace::Index(_)) => Ordering::Greater,
            (WalkPlace::Scalar(left), WalkPlace::Scalar(right)) => left.compare(right),
        }
    }
}

impl From<SortKey> for WalkPlace {
    fn from(place: SortKey) -> Self {
        match place {
            SortKey::Integer(index) if index >= 1 => WalkPlace::Index(index),
            place => WalkPlace::Scalar(place),
        }
    }
}

/// Whether `table` is weak: its metatable has a `__mode` that is a string
/// with `k` or `v` in it before its first zero byte, which is how Lua reads
/// the field. Lua reads it each time it collects, so a table can become
/// weak after it has its metatable.
///
/// A walk that keeps no reference to each entry refuses a weak table.
/// Lua's collector takes an entry out of one once nothing else uses the
/// entry's key or value, whichever the mode names. Such a walk writes to
/// Lua's heap as it reads, and the collector can run at each write: it
/// then takes the entries the walk has not read yet, and keeps the one
/// Rust holds. Which entries those are follows Lua's own order, which
/// differs from one VM to the next, so what the walk found would too.
///
/// # Errors
/// Returns the `mlua` error of the metatable read.
pub(crate) fn is_weak(table: &Table) -> mlua::Result<bool> {
    let Some(metatable) = table.metatable() else {
        return Ok(false);
    };
    Ok(match metatable.raw_get::<Value>("__mode")? {
        Value::String(mode) => mode
            .as_bytes()
            .iter()
            .take_while(|byte| **byte != 0)
            .any(|byte| *byte == b'k' || *byte == b'v'),
        _ => false,
    })
}

/// Puts the `places.len()` items of the Lua list `list` after its first
/// `base` in the order `compare` gives their places: `places[slot]` is the
/// place of the item at `base + slot + 1`.
///
/// For a walk that must hold no Lua reference to each item, of a table
/// that [`is_weak`] says is not weak. `mlua` gives
/// every Lua value a Rust value holds a slot on one stack, and panics when
/// that stack cannot grow, which near the VM's memory limit it cannot. So
/// the walk writes each item to a Lua list as it meets it and keeps only
/// the item's place, the places are sorted here as plain data, and each
/// item then moves to its slot.
///
/// # Errors
/// Returns the `mlua` error of a read or a write of `list`.
pub(crate) fn sort_list<P>(
    list: &Table,
    base: usize,
    places: &[P],
    compare: impl Fn(&P, &P) -> Ordering,
) -> mlua::Result<()> {
    let mut sources = (0..places.len()).collect::<Vec<_>>();
    sources.sort_by(|left, right| compare(&places[*left], &places[*right]));
    arrange(list, base, sources)
}

/// How many items the list in `table` has: the indices from 1 up that
/// hold a value, to the first that holds none.
///
/// Not `#table`. For a table with a gap in those indices Lua's length is
/// any index before a gap, and when the indices sit in the table's hash
/// part Lua picks among them with the seed of the state, which differs
/// from one run to the next. So a walk that split a table at `#table`
/// split it in two ways. The count costs one read for each item.
///
/// # Errors
/// Returns the `mlua` error of a read of `table`.
pub(crate) fn listed(table: &Table) -> mlua::Result<usize> {
    let mut count = 0;
    while !table.raw_get::<Value>(count + 1)?.is_nil() {
        count += 1;
    }
    Ok(count)
}

/// Of the entries a check refuses in one table, the refusal of the entry
/// whose key comes first.
///
/// Lua's own walk meets the entries of a table in an order that differs
/// from one run to the next. A check that returned at the first entry it
/// refused would then name a different entry in each run. A check that
/// sorted the entries first would hold a Lua reference to each of them,
/// and `mlua` panics when a state holds about a million. So the check reads
/// every entry in Lua's order, hands each refusal here, and this keeps the
/// one with the least key and drops the rest.
///
/// The keys go by [`compare_places`]. Two keys without a place and of one
/// type tie, and the refusal met first stays, so the refusal of such a key
/// must say nothing of the key but its type.
pub(crate) struct FirstRefusal<E> {
    least: Option<(Result<SortKey, &'static str>, E)>,
}

impl<E> Default for FirstRefusal<E> {
    fn default() -> Self {
        Self { least: None }
    }
}

impl<E> FirstRefusal<E> {
    /// Records that the check refuses the entry under `key` with `refusal`.
    pub(crate) fn refuse(&mut self, key: &Value, refusal: E) {
        let place = position(key);
        let earlier = self
            .least
            .as_ref()
            .is_none_or(|(least, _)| compare_places(&place, least) == Ordering::Less);
        if earlier {
            self.least = Some((place, refusal));
        }
    }

    /// The refusal of the entry with the least key.
    ///
    /// # Errors
    /// Returns that refusal when the check refused an entry.
    pub(crate) fn into_result(self) -> Result<(), E> {
        self.least.map_or(Ok(()), |(_, refusal)| Err(refusal))
    }
}

/// Reorders the `sources.len()` items of `list` after its first `base`:
/// the item at slot `i` becomes the one that was at slot `sources[i]`.
/// `sources` holds every slot once. Each item moves once, along the cycle
/// of slots it belongs to, so Rust holds two items at a time.
fn arrange(list: &Table, base: usize, mut sources: Vec<usize>) -> mlua::Result<()> {
    let index = |slot: usize| base + slot + 1;
    for start in 0..sources.len() {
        // A slot that already holds its item has itself as its source.
        if sources[start] == start {
            continue;
        }
        let first: Value = list.raw_get(index(start))?;
        let mut slot = start;
        loop {
            let source = sources[slot];
            sources[slot] = slot;
            if source == start {
                list.raw_set(index(slot), first)?;
                break;
            }
            let moved: Value = list.raw_get(index(source))?;
            list.raw_set(index(slot), moved)?;
            slot = source;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    //! Tests for the shared hash-key ordering helper.

    use std::cmp::Ordering;

    use mlua::{Lua, Value};

    use super::{KeyError, SortKey, compare_integer_float, sort_key};

    #[test]
    fn compare_integer_float_orders_without_rounding_the_integer() {
        assert_eq!(compare_integer_float(2, 2.5), Ordering::Less);
        assert_eq!(compare_integer_float(3, 2.5), Ordering::Greater);
        assert_eq!(compare_integer_float(2, 2.0), Ordering::Equal);
        assert_eq!(compare_integer_float(-1, -0.5), Ordering::Less);
        assert_eq!(compare_integer_float(0, -0.5), Ordering::Greater);
        assert_eq!(compare_integer_float(i64::MAX, 1e300), Ordering::Less);
        assert_eq!(compare_integer_float(i64::MIN, -1e300), Ordering::Greater);
        // 2^63 as a float is one past i64::MAX, so the largest integer is
        // still below it; -2^63 is exactly i64::MIN.
        assert_eq!(
            compare_integer_float(i64::MAX, 9_223_372_036_854_775_808.0),
            Ordering::Less
        );
        assert_eq!(
            compare_integer_float(i64::MIN, -9_223_372_036_854_775_808.0),
            Ordering::Equal
        );
    }

    #[test]
    fn sort_key_classifies_scalars_and_rejects_unsortable_keys() {
        let lua = Lua::new();

        let (key, label) = sort_key(&Value::Boolean(false)).expect("a boolean classifies");
        assert!(matches!(key, SortKey::Bool(false)));
        assert_eq!(label, "false");

        // An integer stays an integer: a float classification would tie two
        // distinct integers past 2^53 and hand their order back to `pairs`.
        let (key, label) = sort_key(&Value::Integer(i64::MAX)).expect("an integer classifies");
        assert!(matches!(key, SortKey::Integer(i64::MAX)));
        assert_eq!(label, i64::MAX.to_string());

        let (key, label) = sort_key(&Value::Number(2.5)).expect("a float classifies");
        assert!(matches!(key, SortKey::Float(value) if value.total_cmp(&2.5).is_eq()));
        assert_eq!(label, "2.5");

        let text = lua.create_string("alpha").expect("a string creates");
        let (key, label) = sort_key(&Value::String(text)).expect("a string classifies");
        assert!(matches!(key, SortKey::Text(bytes) if bytes == b"alpha"));
        assert_eq!(label, "alpha");

        // A non-finite number is refused, and each caller decides its own
        // diagnostic.
        let non_finite =
            sort_key(&Value::Number(f64::INFINITY)).expect_err("a non-finite number fails");
        assert!(matches!(non_finite, KeyError::NotFinite));

        // A table key has no cross-type rank, so it is rejected rather than
        // dropped or ordered by an invented comparison.
        let table = lua.create_table().expect("a table creates");
        let unsortable = sort_key(&Value::Table(table)).expect_err("a non-scalar key fails");
        assert!(matches!(unsortable, KeyError::Unsortable("table")));
    }
}
