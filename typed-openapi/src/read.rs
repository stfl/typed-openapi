//! A request body read into the type its operation takes, with the first place
//! where the type skipped a property.
//!
//! A derived `Deserialize` skips a key it has no field for without a word, and
//! `deny_unknown_fields` would refuse the vendor's responses as well, which the
//! same types decode. So the body is read through [`Reader`], which is
//! `serde_json`'s own reading of a [`Value`] with one addition: it knows where it
//! is in the body. Serde asks for `deserialize_ignored_any` when a type skips a
//! value, and the reader notes the place.
//!
//! The place comes from the read itself and not from a search of the body. The
//! key of an externally tagged variant is a step of the body, and only the type
//! knows that a one-key object is a variant: a body can spell the key of a
//! variant like a property of its content. The reader takes the variant key from
//! the same `deserialize_enum` that the type asks, so a path is the body's own
//! whichever way the type read it.
//!
//! Everything but the walk down is `serde_json`'s. A scalar, an error and the
//! count of what a read leaves behind are asked of the `&Value` that `serde_json`
//! implements `Deserializer` for. A list and an object are read by `serde_json`
//! too, through a visitor that hands the type's own visitor an access that knows
//! the key or the index of each value, and the keys of a map are `serde_json`'s
//! (an integer key parses as it does there).

use std::cell::RefCell;
use std::fmt;
use std::iter::Enumerate;
use std::slice;

use serde::Deserializer;
use serde::de::{
    self, DeserializeOwned, DeserializeSeed, EnumAccess, IgnoredAny, MapAccess, SeqAccess,
    VariantAccess, Visitor,
};
use serde_json::map;
use serde_json::{Map, Value};

/// One step into a body: a property of an object, or an element of a list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Step<'a> {
    Key(&'a str),
    Index(usize),
}

/// A body read into `T`.
pub(crate) struct Read<'a, T> {
    /// What the body made of the type.
    pub(crate) value: T,
    /// The place of the first property that the type skipped while it read, if
    /// it skipped one. The key of every externally tagged variant on the way is a
    /// step of it. A type reads some of its values from a buffer, an untagged
    /// union among them, and nothing is skipped from a buffer.
    pub(crate) skipped: Option<Vec<Step<'a>>>,
}

/// Read `body` into `T`. The error names where in the body the read stopped, in
/// the spelling of `serde_path_to_error`.
pub(crate) fn read<T: DeserializeOwned>(
    body: &Value,
) -> Result<Read<'_, T>, serde_path_to_error::Error<serde_json::Error>> {
    let trail = Trail::default();
    let value = serde_path_to_error::deserialize(Reader {
        value: body,
        trail: &trail,
    })?;
    Ok(Read {
        value,
        skipped: trail.skipped.into_inner(),
    })
}

/// Where a read stands in the body, and the first place that the type skipped.
#[derive(Default)]
struct Trail<'a> {
    /// The steps from the body to the value being read.
    here: RefCell<Vec<Step<'a>>>,
    skipped: RefCell<Option<Vec<Step<'a>>>>,
}

impl<'a> Trail<'a> {
    /// Run `read` one step deeper than the trail stands, and leave the trail as it
    /// was whatever `read` answers.
    fn under<R>(&self, step: Step<'a>, read: impl FnOnce() -> R) -> R {
        self.here.borrow_mut().push(step);
        let answer = read();
        self.here.borrow_mut().pop();
        answer
    }

    /// The type skipped the value at the place the trail stands. Only the first
    /// is kept.
    fn skip(&self) {
        let mut skipped = self.skipped.borrow_mut();
        if skipped.is_none() {
            *skipped = Some(self.here.borrow().clone());
        }
    }
}

/// `serde_json`'s reading of a value, which notes what the type skips.
#[derive(Clone, Copy)]
struct Reader<'t, 'de> {
    value: &'de Value,
    trail: &'t Trail<'de>,
}

impl<'de> Reader<'_, 'de> {
    /// Read the list that the value is with `visitor`, giving it a reader for
    /// each element. Any other value is refused as `serde_json` refuses it.
    fn list<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, serde_json::Error> {
        if let Value::Array(items) = self.value {
            self.value.deserialize_seq(Listed {
                visitor,
                items,
                trail: self.trail,
            })
        } else {
            self.value.deserialize_seq(visitor)
        }
    }

    /// Read the object that the value is with `visitor`, giving it a reader for
    /// each property. Any other value is refused as `serde_json` refuses it.
    fn object<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, serde_json::Error> {
        if let Value::Object(entries) = self.value {
            self.value.deserialize_map(Keyed {
                visitor,
                entries,
                trail: self.trail,
            })
        } else {
            self.value.deserialize_map(visitor)
        }
    }
}

/// The variant that an object of one key names, and what it holds.
fn variant_of(entries: &Map<String, Value>) -> Option<(&str, &Value)> {
    entries
        .iter()
        .next()
        .filter(|_| entries.len() == 1)
        .map(|(variant, content)| (variant.as_str(), content))
}

/// A method that `serde_json` answers for the value itself, because a scalar has
/// nothing inside it to place.
macro_rules! of_the_value {
    ($($method:ident)*) => {
        $(
            fn $method<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
                self.value.$method(visitor)
            }
        )*
    };
}

impl<'de> Deserializer<'de> for Reader<'_, 'de> {
    type Error = serde_json::Error;

    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        match self.value {
            Value::Array(_) => self.list(visitor),
            Value::Object(_) => self.object(visitor),
            Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {
                self.value.deserialize_any(visitor)
            }
        }
    }

    fn deserialize_seq<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        self.list(visitor)
    }

    fn deserialize_tuple<V: Visitor<'de>>(
        self,
        _: usize,
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        self.list(visitor)
    }

    fn deserialize_tuple_struct<V: Visitor<'de>>(
        self,
        _: &'static str,
        _: usize,
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        self.list(visitor)
    }

    fn deserialize_map<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        self.object(visitor)
    }

    /// A struct is read from an object, or from a list by place, as `serde_json`
    /// reads it.
    fn deserialize_struct<V: Visitor<'de>>(
        self,
        name: &'static str,
        fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        match self.value {
            Value::Array(_) => self.list(visitor),
            Value::Object(_) => self.object(visitor),
            Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {
                self.value.deserialize_struct(name, fields, visitor)
            }
        }
    }

    /// An object of one key is a variant and its key is a step into the body. A
    /// string is a variant with nothing in it, and any other value is refused as
    /// `serde_json` refuses it.
    fn deserialize_enum<V: Visitor<'de>>(
        self,
        name: &'static str,
        variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        match self.value.as_object().and_then(variant_of) {
            Some((variant, content)) => self.value.deserialize_enum(
                name,
                variants,
                Variants {
                    visitor,
                    variant,
                    content,
                    trail: self.trail,
                },
            ),
            None => self.value.deserialize_enum(name, variants, visitor),
        }
    }

    /// `Some` holds the same value, so it is no step in the body.
    fn deserialize_option<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        if self.value.is_null() {
            visitor.visit_none()
        } else {
            visitor.visit_some(self)
        }
    }

    /// A newtype holds the same value, so it is no step in the body.
    fn deserialize_newtype_struct<V: Visitor<'de>>(
        self,
        _: &'static str,
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        visitor.visit_newtype_struct(self)
    }

    /// Serde asks for this where a type has no field for a key, and that is the
    /// place this module exists to note.
    fn deserialize_ignored_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        self.trail.skip();
        visitor.visit_unit()
    }

    fn deserialize_unit_struct<V: Visitor<'de>>(
        self,
        name: &'static str,
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        self.value.deserialize_unit_struct(name, visitor)
    }

    of_the_value! {
        deserialize_bool deserialize_i8 deserialize_i16 deserialize_i32 deserialize_i64
        deserialize_i128 deserialize_u8 deserialize_u16 deserialize_u32 deserialize_u64
        deserialize_u128 deserialize_f32 deserialize_f64 deserialize_char deserialize_str
        deserialize_string deserialize_bytes deserialize_byte_buf deserialize_unit
        deserialize_identifier
    }
}

/// The visitor that `serde_json` hands a list to. It gives the visitor of the
/// type an access that reads each element through a [`Reader`].
struct Listed<'t, 'de, V> {
    visitor: V,
    items: &'de [Value],
    trail: &'t Trail<'de>,
}

impl<'de, V: Visitor<'de>> Visitor<'de> for Listed<'_, 'de, V> {
    type Value = V::Value;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.visitor.expecting(formatter)
    }

    fn visit_seq<A: SeqAccess<'de>>(self, seq: A) -> Result<Self::Value, A::Error> {
        self.visitor.visit_seq(Elements {
            seq,
            items: self.items.iter().enumerate(),
            trail: self.trail,
        })
    }

    /// `serde_json` reads a tuple variant that holds no elements as a unit.
    fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
        self.visitor.visit_unit()
    }
}

/// `serde_json`'s access to the elements of a list, with a step for each.
struct Elements<'t, 'de, A> {
    seq: A,
    items: Enumerate<slice::Iter<'de, Value>>,
    trail: &'t Trail<'de>,
}

impl<'de, A: SeqAccess<'de>> SeqAccess<'de> for Elements<'_, 'de, A> {
    type Error = A::Error;

    fn next_element_seed<S: DeserializeSeed<'de>>(
        &mut self,
        seed: S,
    ) -> Result<Option<S::Value>, Self::Error> {
        let Some((index, value)) = self.items.next() else {
            return Ok(None);
        };
        // `serde_json` counts the elements it has handed out and refuses a read
        // that leaves any, so its access moves on beside this one.
        self.seq.next_element::<IgnoredAny>()?;
        self.trail
            .under(Step::Index(index), || {
                seed.deserialize(Reader {
                    value,
                    trail: self.trail,
                })
            })
            .map(Some)
            .map_err(<A::Error as de::Error>::custom)
    }

    fn size_hint(&self) -> Option<usize> {
        self.seq.size_hint()
    }
}

/// The visitor that `serde_json` hands an object to. It gives the visitor of the
/// type an access that reads each value through a [`Reader`].
struct Keyed<'t, 'de, V> {
    visitor: V,
    entries: &'de Map<String, Value>,
    trail: &'t Trail<'de>,
}

impl<'de, V: Visitor<'de>> Visitor<'de> for Keyed<'_, 'de, V> {
    type Value = V::Value;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.visitor.expecting(formatter)
    }

    fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<Self::Value, A::Error> {
        self.visitor.visit_map(Pairs {
            map,
            entries: self.entries.iter(),
            current: None,
            trail: self.trail,
        })
    }
}

/// `serde_json`'s access to the properties of an object, with a step for each
/// value. The keys are `serde_json`'s own, so the way it reads a key for a type
/// (an integer, a variant) is the way it is read here.
struct Pairs<'t, 'de, A> {
    map: A,
    entries: map::Iter<'de>,
    /// The property whose key was read last.
    current: Option<(&'de String, &'de Value)>,
    trail: &'t Trail<'de>,
}

impl<'de, A: MapAccess<'de>> MapAccess<'de> for Pairs<'_, 'de, A> {
    type Error = A::Error;

    fn next_key_seed<K: DeserializeSeed<'de>>(
        &mut self,
        seed: K,
    ) -> Result<Option<K::Value>, Self::Error> {
        self.current = self.entries.next();
        self.map.next_key_seed(seed)
    }

    fn next_value_seed<S: DeserializeSeed<'de>>(
        &mut self,
        seed: S,
    ) -> Result<S::Value, Self::Error> {
        // A value asked for before a key is `serde_json`'s error to give.
        let Some((key, value)) = self.current.take() else {
            return self.map.next_value_seed(seed);
        };
        self.trail
            .under(Step::Key(key), || {
                seed.deserialize(Reader {
                    value,
                    trail: self.trail,
                })
            })
            .map_err(<A::Error as de::Error>::custom)
    }

    fn size_hint(&self) -> Option<usize> {
        self.map.size_hint()
    }
}

/// The visitor that `serde_json` hands an object of one key to, which is a
/// variant. The key is a step into the body, so the access it gives the visitor of
/// the type puts the key on the trail while the content is read.
struct Variants<'t, 'de, V> {
    visitor: V,
    variant: &'de str,
    content: &'de Value,
    trail: &'t Trail<'de>,
}

impl<'de, V: Visitor<'de>> Visitor<'de> for Variants<'_, 'de, V> {
    type Value = V::Value;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.visitor.expecting(formatter)
    }

    fn visit_enum<A: EnumAccess<'de>>(self, data: A) -> Result<Self::Value, A::Error> {
        self.visitor.visit_enum(Member {
            data,
            variant: self.variant,
            content: self.content,
            trail: self.trail,
        })
    }
}

/// `serde_json`'s access to the variant of an enum, which hands on the content.
struct Member<'t, 'de, A> {
    data: A,
    variant: &'de str,
    content: &'de Value,
    trail: &'t Trail<'de>,
}

impl<'t, 'de, A: EnumAccess<'de>> EnumAccess<'de> for Member<'t, 'de, A> {
    type Error = A::Error;
    type Variant = Inside<'t, 'de, A::Variant>;

    fn variant_seed<S: DeserializeSeed<'de>>(
        self,
        seed: S,
    ) -> Result<(S::Value, Self::Variant), Self::Error> {
        let (value, access) = self.data.variant_seed(seed)?;
        let inside = Inside {
            access,
            variant: self.variant,
            content: self.content,
            trail: self.trail,
        };
        Ok((value, inside))
    }
}

/// `serde_json`'s access to the content of a variant, read with the key of the
/// variant on the trail.
struct Inside<'t, 'de, V> {
    access: V,
    variant: &'de str,
    content: &'de Value,
    trail: &'t Trail<'de>,
}

impl<'de, V: VariantAccess<'de>> VariantAccess<'de> for Inside<'_, 'de, V> {
    type Error = V::Error;

    fn unit_variant(self) -> Result<(), Self::Error> {
        self.access.unit_variant()
    }

    fn newtype_variant_seed<S: DeserializeSeed<'de>>(
        self,
        seed: S,
    ) -> Result<S::Value, Self::Error> {
        self.trail
            .under(Step::Key(self.variant), || {
                seed.deserialize(Reader {
                    value: self.content,
                    trail: self.trail,
                })
            })
            .map_err(<V::Error as de::Error>::custom)
    }

    fn tuple_variant<W: Visitor<'de>>(
        self,
        len: usize,
        visitor: W,
    ) -> Result<W::Value, Self::Error> {
        self.trail.under(Step::Key(self.variant), || {
            if let Value::Array(items) = self.content {
                self.access.tuple_variant(
                    len,
                    Listed {
                        visitor,
                        items,
                        trail: self.trail,
                    },
                )
            } else {
                self.access.tuple_variant(len, visitor)
            }
        })
    }

    fn struct_variant<W: Visitor<'de>>(
        self,
        fields: &'static [&'static str],
        visitor: W,
    ) -> Result<W::Value, Self::Error> {
        self.trail.under(Step::Key(self.variant), || {
            if let Value::Object(entries) = self.content {
                self.access.struct_variant(
                    fields,
                    Keyed {
                        visitor,
                        entries,
                        trail: self.trail,
                    },
                )
            } else {
                self.access.struct_variant(fields, visitor)
            }
        })
    }
}

#[cfg(test)]
mod tests {
    #![expect(clippy::unwrap_used, reason = "a failed unwrap is a failing test")]

    use std::collections::HashMap;

    use serde::Deserialize;
    use serde_json::{Value, json};

    use super::{DeserializeOwned, Deserializer, MapAccess, SeqAccess, Step, Visitor, fmt, read};

    /// A struct that reads one property and has no field for any other.
    #[derive(Debug, PartialEq, Deserialize)]
    struct Address {
        street: String,
    }

    #[derive(Debug, PartialEq, Deserialize)]
    struct Customer {
        name: String,
        address: Option<Address>,
        #[serde(default)]
        positions: Vec<Address>,
    }

    /// The five shapes of an enum that serde reads, which differ in the access
    /// they ask `serde_json` for.
    #[derive(Debug, PartialEq, Deserialize)]
    enum Place {
        Unit,
        Newtype(Address),
        Pair(Address, Address),
        Record { street: String },
        Empty(),
    }

    /// The step each place in a body leads through, as a test states it.
    fn key(name: &str) -> Step<'_> {
        Step::Key(name)
    }

    /// Where `T` skipped a property while it read `body`, if it did.
    fn skipped<T: DeserializeOwned>(body: &Value) -> Option<Vec<Step<'_>>> {
        read::<T>(body).unwrap().skipped
    }

    #[test]
    fn a_property_the_type_has_no_field_for_is_noted_at_its_place() {
        let top = json!({"name": "Acme Ltd", "zzz": 1});
        let nested = json!({"name": "Acme Ltd", "address": {"street": "s", "extra": 1}});
        let listed = json!({
            "name": "Acme Ltd",
            "positions": [{"street": "s"}, {"street": "t", "extra": 1}],
        });
        let whole = json!({"name": "Acme Ltd", "address": {"street": "s"}});

        assert_eq!(skipped::<Customer>(&top), Some(vec![key("zzz")]));
        assert_eq!(
            skipped::<Customer>(&nested),
            Some(vec![key("address"), key("extra")]),
            "a Some is no step"
        );
        assert_eq!(
            skipped::<Customer>(&listed),
            Some(vec![key("positions"), Step::Index(1), key("extra")])
        );
        assert_eq!(skipped::<Customer>(&whole), None);
    }

    #[test]
    fn only_the_first_property_the_type_skips_is_kept() {
        let body = json!({"name": "Acme Ltd", "first": 1, "second": 2});

        assert_eq!(skipped::<Customer>(&body), Some(vec![key("first")]));
    }

    /// The key of a variant is a step of the body, and serde asks for the access
    /// to the variant that the reader hands on.
    #[test]
    fn the_key_of_a_variant_is_a_step_of_the_place() {
        let newtype = json!({"Newtype": {"street": "s", "extra": 1}});
        let record = json!({"Record": {"street": "s", "extra": 1}});
        let pair = json!({"Pair": [{"street": "s"}, {"street": "t", "extra": 1}]});
        let repeated = json!({"Record": {"street": "s", "Record": 1}});

        assert_eq!(
            skipped::<Place>(&newtype),
            Some(vec![key("Newtype"), key("extra")])
        );
        assert_eq!(
            skipped::<Place>(&record),
            Some(vec![key("Record"), key("extra")])
        );
        assert_eq!(
            skipped::<Place>(&pair),
            Some(vec![key("Pair"), Step::Index(1), key("extra")])
        );
        assert_eq!(
            skipped::<Place>(&repeated),
            Some(vec![key("Record"), key("Record")]),
            "a property spelled like the variant that holds it is a step of its own"
        );
    }

    /// Serde skips nothing from a buffer, so an untagged union and a `flatten`ed
    /// struct note nothing, whatever the body holds.
    #[test]
    fn nothing_is_noted_from_a_buffer() {
        #[derive(Deserialize)]
        #[serde(untagged)]
        #[expect(dead_code, reason = "read only by the deserialiser under test")]
        enum Either {
            Address(Address),
        }

        #[derive(Deserialize)]
        #[expect(dead_code, reason = "read only by the deserialiser under test")]
        struct Composed {
            #[serde(flatten)]
            address: Address,
        }

        let body = json!({"street": "s", "extra": 1});

        assert_eq!(skipped::<Either>(&body), None);
        assert_eq!(skipped::<Composed>(&body), None);
        assert_eq!(skipped::<Address>(&body), Some(vec![key("extra")]));
    }

    /// A type may swallow a failed read of a property and go on with the next. The
    /// places it reads after that are counted from the body, and not from where
    /// the failed read stopped.
    #[test]
    fn a_failed_read_that_the_type_swallows_leaves_the_places_after_it_alone() {
        #[expect(
            clippy::unnecessary_wraps,
            reason = "`deserialize_with` takes a function that answers a `Result`"
        )]
        fn lenient<'de, D: Deserializer<'de>>(de: D) -> Result<Option<Address>, D::Error> {
            Ok(Address::deserialize(de).ok())
        }

        #[derive(Deserialize)]
        #[expect(dead_code, reason = "read only by the deserialiser under test")]
        struct Forgiving {
            #[serde(deserialize_with = "lenient")]
            first: Option<Address>,
            second: Address,
        }

        let body = json!({"first": {"street": 5}, "second": {"street": "s", "extra": 1}});

        assert_eq!(
            skipped::<Forgiving>(&body),
            Some(vec![key("second"), key("extra")])
        );
    }

    /// An integer key is parsed by `serde_json`, which hands the reader the
    /// access to the keys.
    #[test]
    fn a_map_with_integer_keys_is_read_and_its_values_are_placed() {
        let body = json!({"7": {"street": "s", "extra": 1}});

        let read = read::<HashMap<u32, Address>>(&body).unwrap();

        assert_eq!(
            read.value.get(&7),
            Some(&Address {
                street: "s".to_owned()
            })
        );
        assert_eq!(read.skipped, Some(vec![key("7"), key("extra")]));
    }

    #[derive(Debug, Deserialize)]
    #[expect(dead_code, reason = "read only by the deserialiser under test")]
    struct Twin(Address, Address);

    /// A type that reads itself through `deserialize_any` and reads typed values
    /// out of the access its visitor is given.
    #[derive(Debug)]
    struct Dynamic;

    struct Typed;

    impl<'de> Visitor<'de> for Typed {
        type Value = Dynamic;

        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("a map or a list of addresses")
        }

        fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Dynamic, A::Error> {
            while map.next_entry::<String, Address>()?.is_some() {}
            Ok(Dynamic)
        }

        fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Dynamic, A::Error> {
            while seq.next_element::<Address>()?.is_some() {}
            Ok(Dynamic)
        }
    }

    impl<'de> Deserialize<'de> for Dynamic {
        fn deserialize<D: Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
            de.deserialize_any(Typed)
        }
    }

    /// Whichever way a type reads a container, what is inside it is placed.
    #[test]
    fn what_is_inside_a_container_is_placed_whichever_way_the_type_reads_it() {
        let extra = json!({"street": "s", "extra": 1});
        let plain = json!({"street": "s"});
        let by_place = json!(["n", extra]);
        let pair = json!([plain, extra]);
        let map = json!({"k": extra});
        let any_map = json!({"inner": extra});
        let any_list = json!([extra]);
        let element = Step::Index(1);

        assert_eq!(
            skipped::<Customer>(&by_place),
            Some(vec![element, key("extra")]),
            "a struct read from a list by place"
        );
        assert_eq!(skipped::<Wrapper>(&extra), Some(vec![key("extra")]));
        assert_eq!(
            skipped::<(Address, Address)>(&pair),
            Some(vec![element, key("extra")]),
            "a tuple"
        );
        assert_eq!(
            skipped::<Twin>(&pair),
            Some(vec![element, key("extra")]),
            "a tuple struct"
        );
        assert_eq!(
            skipped::<HashMap<String, Address>>(&map),
            Some(vec![key("k"), key("extra")])
        );
        assert_eq!(
            skipped::<Dynamic>(&any_map),
            Some(vec![key("inner"), key("extra")]),
            "an object read through deserialize_any"
        );
        assert_eq!(
            skipped::<Dynamic>(&any_list),
            Some(vec![Step::Index(0), key("extra")]),
            "a list read through deserialize_any"
        );
    }

    /// A struct is read from a list by place, as `serde_json` reads it, and that
    /// skips nothing.
    #[test]
    fn a_struct_is_read_from_a_list_by_place() {
        let body = json!(["s"]);

        let read = read::<Address>(&body).unwrap();

        assert_eq!(
            read.value,
            Address {
                street: "s".to_owned()
            }
        );
        assert_eq!(read.skipped, None);
    }

    /// The value, or the message, that `serde_json` gives for `body`.
    fn serde_json_reads<T: DeserializeOwned>(body: &Value) -> Result<T, String> {
        serde_json::from_value(body.clone()).map_err(|refused| refused.to_string())
    }

    /// The same, through the reader.
    fn reader_reads<T: DeserializeOwned>(body: &Value) -> Result<T, String> {
        read(body)
            .map(|read| read.value)
            .map_err(|refused| refused.into_inner().to_string())
    }

    /// Every body is read into the same value, or refused with the same message,
    /// as `serde_json` reads it: the reader adds a place and changes nothing else.
    fn agrees<T: DeserializeOwned + PartialEq + std::fmt::Debug>(bodies: &[Value]) {
        for body in bodies {
            assert_eq!(
                reader_reads::<T>(body),
                serde_json_reads::<T>(body),
                "{body}"
            );
        }
    }

    #[derive(Debug, PartialEq, Deserialize)]
    struct Scalars {
        flag: bool,
        small: u8,
        wide: i64,
        big: u128,
        real: f64,
        text: String,
        letter: char,
        maybe: Option<String>,
        nothing: (),
        any: Value,
        keyed: HashMap<u32, String>,
        tuple: (i64, String),
        list: Vec<Vec<i64>>,
    }

    /// A newtype that is not `#[serde(transparent)]` asks for a newtype struct.
    #[derive(Debug, PartialEq, Deserialize)]
    struct Wrapper(Address);

    #[derive(Debug, PartialEq, Deserialize)]
    #[serde(tag = "kind")]
    enum Tagged {
        A { x: i64 },
        B,
    }

    #[derive(Debug, PartialEq, Deserialize)]
    #[serde(untagged)]
    enum Loose {
        Number(i64),
        Address(Address),
    }

    #[test]
    fn a_scalar_is_read_as_serde_json_reads_it() {
        let good = json!({
            "flag": true, "small": 7, "wide": -9, "big": 12, "real": 1.5, "text": "t",
            "letter": "c", "maybe": null, "nothing": null, "any": {"k": [1, "two", null]},
            "keyed": {"1": "one", "2": "two"}, "tuple": [1, "x"], "list": [[1], [], [2, 3]],
        });
        let mut bodies = vec![good.clone(), json!([]), json!(5), Value::Null];
        for (name, wrong) in [
            ("flag", json!("true")),
            ("small", json!(300)),
            ("small", json!(1.5)),
            ("wide", json!("9")),
            ("real", json!("x")),
            ("text", json!(5)),
            ("letter", json!("cc")),
            ("nothing", json!(0)),
            ("keyed", json!({"x": "one"})),
            ("keyed", json!([])),
            ("tuple", json!([1])),
            ("tuple", json!([1, "x", 3])),
            ("tuple", json!({"a": 1})),
            ("list", json!([1])),
        ] {
            let mut body = good.clone();
            body.as_object_mut().unwrap().insert(name.to_owned(), wrong);
            bodies.push(body);
        }
        agrees::<Scalars>(&bodies);
    }

    #[test]
    fn an_enum_is_read_as_serde_json_reads_it() {
        agrees::<Place>(&[
            json!("Unit"),
            json!({"Unit": null}),
            json!({"Newtype": {"street": "s"}}),
            json!({"Newtype": ["s"]}),
            json!({"Newtype": "s"}),
            json!({"Pair": [{"street": "s"}, {"street": "t"}]}),
            json!({"Pair": [{"street": "s"}]}),
            json!({"Pair": {"street": "s"}}),
            json!({"Record": {"street": "s"}}),
            json!({"Record": ["s"]}),
            json!({"Record": "s"}),
            json!({"Empty": []}),
            json!({"Empty": [1]}),
            json!({"Nope": 1}),
            json!({"Unit": null, "Newtype": {"street": "s"}}),
            json!("Nope"),
            json!({}),
            json!(5),
            json!(["Unit"]),
        ]);
    }

    #[test]
    fn a_wrapper_a_tagged_union_and_an_untagged_union_are_read_as_serde_json_reads_them() {
        agrees::<Wrapper>(&[json!({"street": "s"}), json!(["s"]), json!("s"), json!([])]);
        agrees::<Tagged>(&[
            json!({"kind": "A", "x": 1}),
            json!({"kind": "B"}),
            json!({"kind": "C"}),
            json!({"x": 1}),
            json!(["A", 1]),
            json!("A"),
        ]);
        agrees::<Loose>(&[json!(5), json!({"street": "s"}), json!("s"), json!([])]);
        agrees::<Customer>(&[
            json!({"name": "n"}),
            json!({"name": "n", "address": null, "positions": []}),
            json!({"name": "n", "positions": {"street": "s"}}),
            json!({"name": "n", "address": ["s"]}),
            json!(["n", null, []]),
            json!(["n"]),
        ]);
    }
}
