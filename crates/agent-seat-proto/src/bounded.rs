//! Allocation-bounded strings and lists.

use std::fmt;
use std::ops::Deref;

use serde::de::{Error as _, IgnoredAny, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// A value exceeded a published byte or item bound.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BoundError {
    limit: usize,
    actual: usize,
}

impl BoundError {
    /// Returns the permitted maximum.
    #[must_use]
    pub const fn limit(self) -> usize {
        self.limit
    }

    /// Returns the rejected size.
    #[must_use]
    pub const fn actual(self) -> usize {
        self.actual
    }
}

impl fmt::Display for BoundError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "value has {} bytes or items; maximum is {}",
            self.actual, self.limit
        )
    }
}

impl std::error::Error for BoundError {}

/// An owned UTF-8 string containing at most `MAX` bytes.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct BoundedText<const MAX: usize>(Box<str>);

impl<const MAX: usize> BoundedText<MAX> {
    /// Creates a bounded string.
    ///
    /// # Errors
    ///
    /// Returns [`BoundError`] when `value` is longer than `MAX` bytes.
    pub fn new(value: impl Into<Box<str>>) -> Result<Self, BoundError> {
        let value = value.into();
        if value.len() > MAX {
            return Err(BoundError {
                limit: MAX,
                actual: value.len(),
            });
        }
        Ok(Self(value))
    }

    /// Returns the string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Consumes the bound wrapper without copying its allocation.
    #[must_use]
    pub fn into_string(self) -> String {
        self.0.into_string()
    }
}

impl<const MAX: usize> Deref for BoundedText<MAX> {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<const MAX: usize> fmt::Display for BoundedText<MAX> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl<const MAX: usize> TryFrom<String> for BoundedText<MAX> {
    type Error = BoundError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value.into_boxed_str())
    }
}

impl<const MAX: usize> Serialize for BoundedText<MAX> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}

impl<'de, const MAX: usize> Deserialize<'de> for BoundedText<MAX> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct TextVisitor<const MAX: usize>;

        impl<const MAX: usize> Visitor<'_> for TextVisitor<MAX> {
            type Value = BoundedText<MAX>;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(formatter, "a UTF-8 string of at most {MAX} bytes")
            }

            fn visit_borrowed_str<E>(self, value: &str) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                BoundedText::new(value).map_err(E::custom)
            }

            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                self.visit_borrowed_str(value)
            }

            fn visit_string<E>(self, value: String) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                BoundedText::try_from(value).map_err(E::custom)
            }
        }

        deserializer.deserialize_str(TextVisitor::<MAX>)
    }
}

/// An owned list containing at most `MAX` items.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BoundedList<T, const MAX: usize>(Box<[T]>);

impl<T, const MAX: usize> BoundedList<T, MAX> {
    /// Creates a bounded list without retaining spare vector capacity.
    ///
    /// # Errors
    ///
    /// Returns [`BoundError`] when `values` contains more than `MAX` items.
    pub fn new(values: Vec<T>) -> Result<Self, BoundError> {
        if values.len() > MAX {
            return Err(BoundError {
                limit: MAX,
                actual: values.len(),
            });
        }
        Ok(Self(values.into_boxed_slice()))
    }

    /// Returns the list as a slice.
    #[must_use]
    pub fn as_slice(&self) -> &[T] {
        &self.0
    }
}

impl<T, const MAX: usize> Default for BoundedList<T, MAX> {
    fn default() -> Self {
        Self(Box::new([]))
    }
}

impl<T, const MAX: usize> Deref for BoundedList<T, MAX> {
    type Target = [T];

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T, const MAX: usize> TryFrom<Vec<T>> for BoundedList<T, MAX> {
    type Error = BoundError;

    fn try_from(value: Vec<T>) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl<T: Serialize, const MAX: usize> Serialize for BoundedList<T, MAX> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.0.serialize(serializer)
    }
}

impl<'de, T: Deserialize<'de>, const MAX: usize> Deserialize<'de> for BoundedList<T, MAX> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct ListVisitor<T, const MAX: usize>(std::marker::PhantomData<T>);

        impl<'de, T: Deserialize<'de>, const MAX: usize> Visitor<'de> for ListVisitor<T, MAX> {
            type Value = BoundedList<T, MAX>;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(formatter, "a list containing at most {MAX} items")
            }

            fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
            where
                A: SeqAccess<'de>,
            {
                if sequence.size_hint().is_some_and(|size| size > MAX) {
                    return Err(A::Error::custom("list exceeds its item bound"));
                }
                let mut values = Vec::with_capacity(sequence.size_hint().unwrap_or(0).min(MAX));
                while values.len() < MAX {
                    let Some(value) = sequence.next_element()? else {
                        return Ok(BoundedList(values.into_boxed_slice()));
                    };
                    values.push(value);
                }
                if sequence.next_element::<IgnoredAny>()?.is_some() {
                    return Err(A::Error::custom("list exceeds its item bound"));
                }
                Ok(BoundedList(values.into_boxed_slice()))
            }
        }

        deserializer.deserialize_seq(ListVisitor::<T, MAX>(std::marker::PhantomData))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_bounds_bytes_not_characters() {
        assert!(BoundedText::<3>::new("abc").is_ok());
        assert!(BoundedText::<3>::new("éé").is_err());
    }

    #[test]
    fn list_deserialization_stops_at_the_bound() {
        let error = serde_json::from_str::<BoundedList<u8, 2>>("[1,2,3]")
            .expect_err("third item must be refused");
        assert!(error.to_string().contains("item bound"));
    }
}
