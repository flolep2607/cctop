//! Typed readers for JSON that other programs write and may change at will.
//!
//! The transcripts cctop reads, and its own hook wire, come from tools that
//! change their schema without notice — and from older and newer copies of
//! cctop itself. The hand-walked `serde_json::Value` these replace had one
//! property worth keeping: a field of the wrong type read as absent, and the
//! rest of the record was still used. A plain `#[derive(Deserialize)]` loses
//! that, because one ill-typed field fails the whole record. The wrappers here
//! put it back without building a `Value`:
//!
//! - [`Lenient`] for a scalar or a list: a value that is not a `T` is `None`.
//! - [`Str`] for a string, borrowed from the input when it has no escapes.
//! - [`Object`] for a nested struct: anything but a JSON object is `None`.
//! - [`Shape`] for the fields that are a string in one record and a list of
//!   blocks in the next, as message content is.
//! - [`Raw`] for a field only some readers want, or a big one: left as the
//!   text it was written as, which any value is, until a reader builds it.
//!
//! What they share: whatever the value turns out to be, it is consumed — an
//! unexpected object or list is skipped with [`IgnoredAny`], which walks it
//! without allocating — so the reader carries on at the next field. Only JSON
//! that does not parse fails, which is what failed before too.
//!
//! A struct built from these is all-lenient only if every field is: a bare
//! `bool` or `String` field still refuses the whole record when it is wrong.
//! Unknown fields are skipped by serde's derive already, and `#[serde(default)]`
//! on the struct makes every field optional.
//!
//! ponytail: a key written twice in one object refuses that object, where the
//! `Value` walk took the last copy. The harnesses write their records with a
//! JSON serialiser, which cannot repeat a key.

use serde::de::value::{
    BoolDeserializer, BorrowedStrDeserializer, F64Deserializer, I64Deserializer,
    MapAccessDeserializer, SeqAccessDeserializer, StrDeserializer, StringDeserializer,
    U64Deserializer, UnitDeserializer,
};
use serde::de::{self, Deserialize, Deserializer, IgnoredAny, MapAccess, SeqAccess, Visitor};
use serde_json::value::RawValue;
use std::borrow::Cow;
use std::fmt;
use std::marker::PhantomData;

/// A value that reads as absent, rather than failing the record it is in,
/// when it is not a `T`.
///
/// For scalars and lists. A struct goes in an [`Object`] instead: serde's
/// derive also takes a struct positionally from a JSON array, which would read
/// `["a", "b"]` as a record whose first two fields are set.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Lenient<T>(pub Option<T>);

impl<T> Default for Lenient<T> {
    fn default() -> Self {
        Lenient(None)
    }
}

impl<T> Lenient<T> {
    pub(crate) fn get(&self) -> Option<&T> {
        self.0.as_ref()
    }
}

impl<T: Copy> Lenient<T> {
    pub(crate) fn value(&self) -> Option<T> {
        self.0
    }
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for Lenient<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(LenientVisitor(PhantomData))
    }
}

struct LenientVisitor<T>(PhantomData<T>);

/// The error a value is read through before it is thrown away: which error it
/// was never matters, only that there was one.
type Discard = de::value::Error;

impl<'de, T: Deserialize<'de>> Visitor<'de> for LenientVisitor<T> {
    type Value = Lenient<T>;

    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("any JSON value")
    }

    fn visit_bool<E>(self, v: bool) -> Result<Self::Value, E> {
        Ok(Lenient(
            T::deserialize(BoolDeserializer::<Discard>::new(v)).ok(),
        ))
    }

    fn visit_i64<E>(self, v: i64) -> Result<Self::Value, E> {
        Ok(Lenient(
            T::deserialize(I64Deserializer::<Discard>::new(v)).ok(),
        ))
    }

    fn visit_u64<E>(self, v: u64) -> Result<Self::Value, E> {
        Ok(Lenient(
            T::deserialize(U64Deserializer::<Discard>::new(v)).ok(),
        ))
    }

    fn visit_f64<E>(self, v: f64) -> Result<Self::Value, E> {
        Ok(Lenient(
            T::deserialize(F64Deserializer::<Discard>::new(v)).ok(),
        ))
    }

    fn visit_borrowed_str<E>(self, v: &'de str) -> Result<Self::Value, E> {
        Ok(Lenient(
            T::deserialize(BorrowedStrDeserializer::<Discard>::new(v)).ok(),
        ))
    }

    fn visit_str<E>(self, v: &str) -> Result<Self::Value, E> {
        Ok(Lenient(
            T::deserialize(StrDeserializer::<Discard>::new(v)).ok(),
        ))
    }

    fn visit_string<E>(self, v: String) -> Result<Self::Value, E> {
        Ok(Lenient(
            T::deserialize(StringDeserializer::<Discard>::new(v)).ok(),
        ))
    }

    fn visit_unit<E>(self) -> Result<Self::Value, E> {
        Ok(Lenient(
            T::deserialize(UnitDeserializer::<Discard>::new()).ok(),
        ))
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
        let got = T::deserialize(SeqAccessDeserializer::new(&mut seq)).ok();
        drain_seq(seq)?;
        Ok(Lenient(got))
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
        let got = T::deserialize(MapAccessDeserializer::new(&mut map)).ok();
        drain_map(map)?;
        Ok(Lenient(got))
    }
}

/// Skip what is left of a list a reader refused or stopped short in.
fn drain_seq<'de, A: SeqAccess<'de>>(mut seq: A) -> Result<(), A::Error> {
    while seq.next_element::<IgnoredAny>()?.is_some() {}
    Ok(())
}

/// Skip what is left of an object a reader refused or stopped short in.
fn drain_map<'de, A: MapAccess<'de>>(mut map: A) -> Result<(), A::Error> {
    while map.next_entry::<IgnoredAny, IgnoredAny>()?.is_some() {}
    Ok(())
}

/// A string field, borrowed from the line when it can be.
///
/// Borrowed when the JSON string has no escapes in it, and owned when it does,
/// since then the text has to be rebuilt. Anything that is not a string is
/// `None`.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct Str<'a>(pub Option<Cow<'a, str>>);

impl<'a> Str<'a> {
    pub(crate) fn get(&self) -> Option<&str> {
        self.0.as_deref()
    }

    /// The string, or `""` when there is none — the `unwrap_or("")` every
    /// reader of an optional field writes.
    pub(crate) fn or_empty(&self) -> &str {
        self.get().unwrap_or("")
    }

    /// Whether the field is this string, exactly.
    pub(crate) fn is(&self, want: &str) -> bool {
        self.get() == Some(want)
    }
}

impl<'de: 'a, 'a> Deserialize<'de> for Str<'a> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(StrVisitor(PhantomData))
    }
}

struct StrVisitor<'a>(PhantomData<&'a ()>);

impl<'de: 'a, 'a> Visitor<'de> for StrVisitor<'a> {
    type Value = Str<'a>;

    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("any JSON value")
    }

    fn visit_borrowed_str<E>(self, v: &'de str) -> Result<Self::Value, E> {
        Ok(Str(Some(Cow::Borrowed(v))))
    }

    fn visit_str<E>(self, v: &str) -> Result<Self::Value, E> {
        Ok(Str(Some(Cow::Owned(v.to_string()))))
    }

    fn visit_string<E>(self, v: String) -> Result<Self::Value, E> {
        Ok(Str(Some(Cow::Owned(v))))
    }

    fn visit_bool<E>(self, _: bool) -> Result<Self::Value, E> {
        Ok(Str(None))
    }

    fn visit_i64<E>(self, _: i64) -> Result<Self::Value, E> {
        Ok(Str(None))
    }

    fn visit_u64<E>(self, _: u64) -> Result<Self::Value, E> {
        Ok(Str(None))
    }

    fn visit_f64<E>(self, _: f64) -> Result<Self::Value, E> {
        Ok(Str(None))
    }

    fn visit_unit<E>(self) -> Result<Self::Value, E> {
        Ok(Str(None))
    }

    fn visit_seq<A: SeqAccess<'de>>(self, seq: A) -> Result<Self::Value, A::Error> {
        drain_seq(seq)?;
        Ok(Str(None))
    }

    fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<Self::Value, A::Error> {
        drain_map(map)?;
        Ok(Str(None))
    }
}

/// A nested record, present only when the value is a JSON object.
///
/// `T` should be built from the wrappers in this module, so that reading it
/// cannot fail on a type. If it does fail anyway the object is skipped and the
/// field is `None`, rather than the record around it being lost.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Object<T>(pub Option<T>);

impl<T> Default for Object<T> {
    fn default() -> Self {
        Object(None)
    }
}

impl<T> Object<T> {
    pub(crate) fn get(&self) -> Option<&T> {
        self.0.as_ref()
    }
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for Object<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(ObjectVisitor(PhantomData))
    }
}

struct ObjectVisitor<T>(PhantomData<T>);

impl<'de, T: Deserialize<'de>> Visitor<'de> for ObjectVisitor<T> {
    type Value = Object<T>;

    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("any JSON value")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
        let got = T::deserialize(MapAccessDeserializer::new(&mut map)).ok();
        drain_map(map)?;
        Ok(Object(got))
    }

    fn visit_seq<A: SeqAccess<'de>>(self, seq: A) -> Result<Self::Value, A::Error> {
        drain_seq(seq)?;
        Ok(Object(None))
    }

    fn visit_bool<E>(self, _: bool) -> Result<Self::Value, E> {
        Ok(Object(None))
    }

    fn visit_i64<E>(self, _: i64) -> Result<Self::Value, E> {
        Ok(Object(None))
    }

    fn visit_u64<E>(self, _: u64) -> Result<Self::Value, E> {
        Ok(Object(None))
    }

    fn visit_f64<E>(self, _: f64) -> Result<Self::Value, E> {
        Ok(Object(None))
    }

    fn visit_str<E>(self, _: &str) -> Result<Self::Value, E> {
        Ok(Object(None))
    }

    fn visit_unit<E>(self) -> Result<Self::Value, E> {
        Ok(Object(None))
    }
}

/// A field that is a string in some records and a list in others, with
/// objects inside the list: message content, a tool result, a queued prompt.
///
/// Every case a reader of the old `Value` matched on is kept apart, so each one
/// can go on meaning what it meant. A bare object at the top is `Object`, which
/// most readers ignore, as they ignored it before.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) enum Shape<'a, T> {
    Text(Cow<'a, str>),
    Object(T),
    List(Vec<Shape<'a, T>>),
    /// A number, a bool, null, an object `T` could not be read from, or the
    /// field missing altogether.
    #[default]
    Other,
}

impl<'a, T> Shape<'a, T> {
    /// The elements, when this is a list.
    pub(crate) fn list(&self) -> Option<&[Shape<'a, T>]> {
        match self {
            Shape::List(items) => Some(items),
            _ => None,
        }
    }

    pub(crate) fn text(&self) -> Option<&str> {
        match self {
            Shape::Text(s) => Some(s),
            _ => None,
        }
    }

    pub(crate) fn object(&self) -> Option<&T> {
        match self {
            Shape::Object(t) => Some(t),
            _ => None,
        }
    }
}

impl<'de: 'a, 'a, T: Deserialize<'de>> Deserialize<'de> for Shape<'a, T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(ShapeVisitor(PhantomData))
    }
}

struct ShapeVisitor<'a, T>(PhantomData<(&'a (), T)>);

impl<'de: 'a, 'a, T: Deserialize<'de>> Visitor<'de> for ShapeVisitor<'a, T> {
    type Value = Shape<'a, T>;

    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("any JSON value")
    }

    fn visit_borrowed_str<E>(self, v: &'de str) -> Result<Self::Value, E> {
        Ok(Shape::Text(Cow::Borrowed(v)))
    }

    fn visit_str<E>(self, v: &str) -> Result<Self::Value, E> {
        Ok(Shape::Text(Cow::Owned(v.to_string())))
    }

    fn visit_string<E>(self, v: String) -> Result<Self::Value, E> {
        Ok(Shape::Text(Cow::Owned(v)))
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
        let got = T::deserialize(MapAccessDeserializer::new(&mut map)).ok();
        drain_map(map)?;
        Ok(got.map_or(Shape::Other, Shape::Object))
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
        let mut items = Vec::new();
        while let Some(item) = seq.next_element::<Shape<'a, T>>()? {
            items.push(item);
        }
        Ok(Shape::List(items))
    }

    fn visit_bool<E>(self, _: bool) -> Result<Self::Value, E> {
        Ok(Shape::Other)
    }

    fn visit_i64<E>(self, _: i64) -> Result<Self::Value, E> {
        Ok(Shape::Other)
    }

    fn visit_u64<E>(self, _: u64) -> Result<Self::Value, E> {
        Ok(Shape::Other)
    }

    fn visit_f64<E>(self, _: f64) -> Result<Self::Value, E> {
        Ok(Shape::Other)
    }

    fn visit_unit<E>(self) -> Result<Self::Value, E> {
        Ok(Shape::Other)
    }
}

/// A JSON value left as the text it was written as, to be read later or not
/// at all.
///
/// For a field only some readers want, or one that is big: the slice of the
/// line is kept, and each reader builds what it needs from it — a `Value`, a
/// typed struct, a length — or nothing. Any JSON is a `Raw`, so the field can
/// never be the wrong type.
///
/// `None` only when the field is missing: a `null` is `Some`, as `Value` kept
/// it, because some readers tell the two apart.
///
/// Only from a reader that borrows — `serde_json::from_slice` or `from_str`
/// — since the text is the input's own.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Raw<'a>(pub Option<&'a RawValue>);

impl<'de: 'a, 'a> Deserialize<'de> for Raw<'a> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        <&'a RawValue>::deserialize(deserializer).map(|raw| Raw(Some(raw)))
    }
}

impl<'a> Raw<'a> {
    /// The value built, or `None` when the field is missing.
    pub(crate) fn to_value(self) -> Option<serde_json::Value> {
        self.0.and_then(|raw| serde_json::from_str(raw.get()).ok())
    }

    /// The value built, with `Null` for a missing field: what
    /// `get(..).cloned().unwrap_or(Value::Null)` gave.
    pub(crate) fn value(self) -> serde_json::Value {
        self.to_value().unwrap_or(serde_json::Value::Null)
    }

    /// Read the value as a `T`, if it is one.
    pub(crate) fn read<T: Deserialize<'a>>(self) -> Option<T> {
        serde_json::from_str(self.0?.get()).ok()
    }

    /// The value's size as a share of the context window: a string's own
    /// length, nothing for `null` or a missing field, and the compact JSON of
    /// anything structured, which is what the API is handed.
    pub(crate) fn content_chars(self) -> u64 {
        let Some(raw) = self.0 else {
            return 0;
        };
        match raw.get().as_bytes().first() {
            Some(b'"') => serde_json::from_str::<TextLen>(raw.get()).map_or(0, |n| n.0),
            Some(b'n') => 0,
            _ => compact_len(raw),
        }
    }
}

/// The length of a value as `serde_json` would write it compactly, measured
/// without building it.
///
/// What [`serde_json::Value::to_string`]`().len()` returned, for the readers
/// that size a piece of content as the JSON the API is handed. The source text
/// is not the same number: it may carry whitespace or escapes the serialiser
/// would not write.
pub(crate) fn compact_len(raw: &serde_json::value::RawValue) -> u64 {
    serde_json::from_str::<CompactLen>(raw.get()).map_or(0, |n| n.0)
}

/// See [`compact_len`].
pub(crate) struct CompactLen(pub u64);

impl<'de> Deserialize<'de> for CompactLen {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(CompactLenVisitor)
    }
}

struct CompactLenVisitor;

/// Bytes `serde_json` writes for a string, quotes included.
fn quoted_len(s: &str) -> u64 {
    let escapes: u64 = s
        .bytes()
        .map(|b| match b {
            b'"' | b'\\' | b'\x08' | b'\x0c' | b'\n' | b'\r' | b'\t' => 1,
            0x00..=0x1f => 5,
            _ => 0,
        })
        .sum();
    s.len() as u64 + escapes + 2
}

fn digits(mut n: u64) -> u64 {
    let mut len = 1;
    while n >= 10 {
        n /= 10;
        len += 1;
    }
    len
}

impl<'de> Visitor<'de> for CompactLenVisitor {
    type Value = CompactLen;

    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("any JSON value")
    }

    fn visit_bool<E>(self, v: bool) -> Result<Self::Value, E> {
        Ok(CompactLen(if v { 4 } else { 5 }))
    }

    fn visit_i64<E>(self, v: i64) -> Result<Self::Value, E> {
        Ok(CompactLen(digits(v.unsigned_abs()) + u64::from(v < 0)))
    }

    fn visit_u64<E>(self, v: u64) -> Result<Self::Value, E> {
        Ok(CompactLen(digits(v)))
    }

    fn visit_f64<E>(self, v: f64) -> Result<Self::Value, E> {
        // Rare in content, and ryu's rules for where the exponent goes are
        // not worth restating to save one small string.
        Ok(CompactLen(
            serde_json::to_string(&v).map_or(0, |s| s.len() as u64),
        ))
    }

    fn visit_str<E>(self, v: &str) -> Result<Self::Value, E> {
        Ok(CompactLen(quoted_len(v)))
    }

    fn visit_unit<E>(self) -> Result<Self::Value, E> {
        Ok(CompactLen(4))
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
        let (mut len, mut n) = (2u64, 0u64);
        while let Some(CompactLen(item)) = seq.next_element()? {
            len += item;
            n += 1;
        }
        Ok(CompactLen(len + n.saturating_sub(1)))
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
        let (mut len, mut n) = (2u64, 0u64);
        while let Some(key) = map.next_key::<KeyLen>()? {
            let CompactLen(value) = map.next_value()?;
            // `"key":value`
            len += key.0 + 1 + value;
            n += 1;
        }
        Ok(CompactLen(len + n.saturating_sub(1)))
    }
}

/// A JSON string's length once its escapes are undone, without building it:
/// what `str::len` of the decoded text is.
pub(crate) struct TextLen(pub u64);

impl<'de> Deserialize<'de> for TextLen {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_str(TextLenVisitor)
    }
}

struct TextLenVisitor;

impl<'de> Visitor<'de> for TextLenVisitor {
    type Value = TextLen;

    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("a string")
    }

    fn visit_str<E>(self, v: &str) -> Result<Self::Value, E> {
        Ok(TextLen(v.len() as u64))
    }
}

/// An object key's written length, without allocating it.
struct KeyLen(u64);

impl<'de> Deserialize<'de> for KeyLen {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_str(KeyLenVisitor)
    }
}

struct KeyLenVisitor;

impl<'de> Visitor<'de> for KeyLenVisitor {
    type Value = KeyLen;

    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("an object key")
    }

    fn visit_str<E>(self, v: &str) -> Result<Self::Value, E> {
        Ok(KeyLen(quoted_len(v)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::value::RawValue;

    #[derive(Debug, Default, serde::Deserialize)]
    #[serde(default)]
    struct Record<'a> {
        #[serde(borrow)]
        name: Str<'a>,
        count: Lenient<u64>,
        flag: Lenient<bool>,
        #[serde(borrow)]
        tags: Lenient<Vec<Str<'a>>>,
        inner: Object<Inner>,
        last: Lenient<u64>,
    }

    #[derive(Debug, Default, serde::Deserialize)]
    #[serde(default)]
    struct Inner {
        n: Lenient<u64>,
    }

    fn read(json: &str) -> Record<'_> {
        serde_json::from_str(json).expect("a record whatever its fields hold")
    }

    #[test]
    fn fields_of_the_right_type_are_read() {
        let r =
            read(r#"{"name":"a","count":3,"flag":true,"tags":["x","y"],"inner":{"n":1},"last":9}"#);
        assert_eq!(r.name.get(), Some("a"));
        assert_eq!(r.count.value(), Some(3));
        assert_eq!(r.flag.value(), Some(true));
        let tags: Vec<_> = r.tags.get().unwrap().iter().filter_map(Str::get).collect();
        assert_eq!(tags, ["x", "y"]);
        assert_eq!(r.inner.get().and_then(|i| i.n.value()), Some(1));
        assert_eq!(r.last.value(), Some(9));
    }

    #[test]
    fn unescaped_strings_are_borrowed_and_escaped_ones_are_not() {
        let r = read(r#"{"name":"plain"}"#);
        assert!(matches!(r.name.0, Some(Cow::Borrowed("plain"))));
        let r = read(r#"{"name":"two\nlines"}"#);
        assert!(matches!(r.name.0, Some(Cow::Owned(ref s)) if s == "two\nlines"));
    }

    /// Each field holds the wrong kind of value, and some of them hold
    /// structures that have to be walked past. Every one reads as absent, and
    /// the field after them all is still read.
    #[test]
    fn a_wrong_typed_field_is_absent_and_the_rest_are_read() {
        let r = read(
            r#"{"name":{"deep":[1,{"x":"y"}]},"count":"3","flag":1,"tags":"x","inner":[1,2],"last":9}"#,
        );
        assert_eq!(r.name.get(), None);
        assert_eq!(r.count.value(), None);
        assert_eq!(r.flag.value(), None);
        assert!(r.tags.get().is_none());
        assert!(r.inner.get().is_none());
        assert_eq!(r.last.value(), Some(9));
    }

    /// What `Value::as_u64` refused, this refuses too.
    #[test]
    fn a_count_must_be_a_whole_non_negative_number() {
        assert_eq!(read(r#"{"count":-1}"#).count.value(), None);
        assert_eq!(read(r#"{"count":1.5}"#).count.value(), None);
        assert_eq!(read(r#"{"count":1e30}"#).count.value(), None);
        assert_eq!(read(r#"{"count":null}"#).count.value(), None);
    }

    #[test]
    fn a_wrong_element_is_absent_and_its_neighbours_are_kept() {
        let r = read(r#"{"tags":["x",2,{"a":1},"y",null]}"#);
        let tags: Vec<_> = r.tags.get().unwrap().iter().map(Str::get).collect();
        assert_eq!(tags, [Some("x"), None, None, Some("y"), None]);
    }

    #[test]
    fn unknown_and_missing_fields_change_nothing() {
        let r = read(r#"{"something":{"new":[true]},"count":2}"#);
        assert_eq!(r.count.value(), Some(2));
        assert_eq!(r.name.get(), None);
        assert!(r.inner.get().is_none());
    }

    #[test]
    fn json_that_does_not_parse_still_fails() {
        assert!(serde_json::from_str::<Record>(r#"{"name":{"x":}"#).is_err());
        assert!(serde_json::from_str::<Record>(r#"{"count":[1,"#).is_err());
    }

    #[test]
    fn a_shape_keeps_text_lists_and_objects_apart() {
        let s: Shape<Inner> = serde_json::from_str(r#""hi""#).unwrap();
        assert_eq!(s.text(), Some("hi"));
        let s: Shape<Inner> = serde_json::from_str(r#"["a",{"n":2},3,[4]]"#).unwrap();
        let items = s.list().unwrap();
        assert_eq!(items[0].text(), Some("a"));
        assert_eq!(items[1].object().and_then(|i| i.n.value()), Some(2));
        assert!(matches!(items[2], Shape::Other));
        assert!(matches!(items[3], Shape::List(_)));
        let s: Shape<Inner> = serde_json::from_str("7").unwrap();
        assert!(matches!(s, Shape::Other));
    }

    /// The length has to be the one `Value` would have written, because the
    /// context meter's figures were fitted against it.
    #[test]
    fn compact_len_is_what_value_wrote() {
        for json in [
            r#"{"a": 1, "b": [true, false, null], "c": {"d": "e\"f\\g\nh\u0001"}}"#,
            r#"[ -12, 0, 18446744073709551615, 1.5, 1e20, -0.25 ]"#,
            r#""café \/ ok""#,
            r#"{}"#,
            r#"[]"#,
            r#"{"k\tey": [ {}, [] ]}"#,
        ] {
            let raw: &RawValue = serde_json::from_str(json).unwrap();
            let value: serde_json::Value = serde_json::from_str(json).unwrap();
            assert_eq!(compact_len(raw), value.to_string().len() as u64, "{json}");
        }
    }
}
