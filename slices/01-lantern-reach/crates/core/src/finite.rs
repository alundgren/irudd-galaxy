//! Checks that a value holds no NaN or infinite numbers.
//!
//! JSON has no way to write them (`serde_json` writes `null`), so a non-finite float
//! anywhere in the state makes a save that can't be loaded again. Commands, authored
//! data and saves are checked with [`check`] before they're accepted.

use std::fmt;

use serde::Serialize;
use serde::ser;

/// `Err` naming the first non-finite number in `value`, e.g. `position.x is NaN`.
pub fn check<T: Serialize + ?Sized>(value: &T) -> Result<(), String> {
    let mut checker = Checker { path: Vec::new() };
    value.serialize(&mut checker).map_err(|e| e.0)
}

struct Checker {
    path: Vec<&'static str>,
}

#[derive(Debug)]
struct NonFinite(String);

impl fmt::Display for NonFinite {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for NonFinite {}

impl ser::Error for NonFinite {
    fn custom<T: fmt::Display>(msg: T) -> Self {
        NonFinite(msg.to_string())
    }
}

impl Checker {
    fn float(&self, v: f64) -> Result<(), NonFinite> {
        if v.is_finite() {
            return Ok(());
        }
        let at = if self.path.is_empty() {
            "value".to_owned()
        } else {
            self.path.join(".")
        };
        Err(NonFinite(format!("{at} is {v}")))
    }

    fn nested<T: Serialize + ?Sized>(
        &mut self,
        name: &'static str,
        value: &T,
    ) -> Result<(), NonFinite> {
        self.path.push(name);
        value.serialize(&mut *self)?;
        self.path.pop();
        Ok(())
    }
}

macro_rules! ignore {
    ($($method:ident: $ty:ty),* $(,)?) => {
        $(fn $method(self, _: $ty) -> Result<(), NonFinite> { Ok(()) })*
    };
}

impl ser::Serializer for &mut Checker {
    type Ok = ();
    type Error = NonFinite;
    type SerializeSeq = Self;
    type SerializeTuple = Self;
    type SerializeTupleStruct = Self;
    type SerializeTupleVariant = Self;
    type SerializeMap = Self;
    type SerializeStruct = Self;
    type SerializeStructVariant = Self;

    ignore! {
        serialize_bool: bool, serialize_i8: i8, serialize_i16: i16, serialize_i32: i32,
        serialize_i64: i64, serialize_i128: i128, serialize_u8: u8, serialize_u16: u16,
        serialize_u32: u32, serialize_u64: u64, serialize_u128: u128, serialize_char: char,
        serialize_str: &str, serialize_bytes: &[u8], serialize_unit_struct: &'static str,
    }

    fn serialize_f32(self, v: f32) -> Result<(), NonFinite> {
        self.float(f64::from(v))
    }
    fn serialize_f64(self, v: f64) -> Result<(), NonFinite> {
        self.float(v)
    }
    fn serialize_none(self) -> Result<(), NonFinite> {
        Ok(())
    }
    fn serialize_some<T: Serialize + ?Sized>(self, value: &T) -> Result<(), NonFinite> {
        value.serialize(self)
    }
    fn serialize_unit(self) -> Result<(), NonFinite> {
        Ok(())
    }
    fn serialize_unit_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
    ) -> Result<(), NonFinite> {
        Ok(())
    }
    fn serialize_newtype_struct<T: Serialize + ?Sized>(
        self,
        _: &'static str,
        value: &T,
    ) -> Result<(), NonFinite> {
        value.serialize(self)
    }
    fn serialize_newtype_variant<T: Serialize + ?Sized>(
        self,
        _: &'static str,
        _: u32,
        variant: &'static str,
        value: &T,
    ) -> Result<(), NonFinite> {
        self.nested(variant, value)
    }
    fn serialize_seq(self, _: Option<usize>) -> Result<Self, NonFinite> {
        Ok(self)
    }
    fn serialize_tuple(self, _: usize) -> Result<Self, NonFinite> {
        Ok(self)
    }
    fn serialize_tuple_struct(self, _: &'static str, _: usize) -> Result<Self, NonFinite> {
        Ok(self)
    }
    fn serialize_tuple_variant(
        self,
        _: &'static str,
        _: u32,
        variant: &'static str,
        _: usize,
    ) -> Result<Self, NonFinite> {
        self.path.push(variant);
        Ok(self)
    }
    fn serialize_map(self, _: Option<usize>) -> Result<Self, NonFinite> {
        Ok(self)
    }
    fn serialize_struct(self, _: &'static str, _: usize) -> Result<Self, NonFinite> {
        Ok(self)
    }
    fn serialize_struct_variant(
        self,
        _: &'static str,
        _: u32,
        variant: &'static str,
        _: usize,
    ) -> Result<Self, NonFinite> {
        self.path.push(variant);
        Ok(self)
    }
}

impl ser::SerializeSeq for &mut Checker {
    type Ok = ();
    type Error = NonFinite;
    fn serialize_element<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), NonFinite> {
        value.serialize(&mut **self)
    }
    fn end(self) -> Result<(), NonFinite> {
        Ok(())
    }
}

impl ser::SerializeTuple for &mut Checker {
    type Ok = ();
    type Error = NonFinite;
    fn serialize_element<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), NonFinite> {
        value.serialize(&mut **self)
    }
    fn end(self) -> Result<(), NonFinite> {
        Ok(())
    }
}

impl ser::SerializeTupleStruct for &mut Checker {
    type Ok = ();
    type Error = NonFinite;
    fn serialize_field<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), NonFinite> {
        value.serialize(&mut **self)
    }
    fn end(self) -> Result<(), NonFinite> {
        Ok(())
    }
}

impl ser::SerializeTupleVariant for &mut Checker {
    type Ok = ();
    type Error = NonFinite;
    fn serialize_field<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), NonFinite> {
        value.serialize(&mut **self)
    }
    fn end(self) -> Result<(), NonFinite> {
        self.path.pop();
        Ok(())
    }
}

impl ser::SerializeMap for &mut Checker {
    type Ok = ();
    type Error = NonFinite;
    fn serialize_key<T: Serialize + ?Sized>(&mut self, key: &T) -> Result<(), NonFinite> {
        key.serialize(&mut **self)
    }
    fn serialize_value<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), NonFinite> {
        value.serialize(&mut **self)
    }
    fn end(self) -> Result<(), NonFinite> {
        Ok(())
    }
}

impl ser::SerializeStruct for &mut Checker {
    type Ok = ();
    type Error = NonFinite;
    fn serialize_field<T: Serialize + ?Sized>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> Result<(), NonFinite> {
        self.nested(key, value)
    }
    fn end(self) -> Result<(), NonFinite> {
        Ok(())
    }
}

impl ser::SerializeStructVariant for &mut Checker {
    type Ok = ();
    type Error = NonFinite;
    fn serialize_field<T: Serialize + ?Sized>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> Result<(), NonFinite> {
        self.nested(key, value)
    }
    fn end(self) -> Result<(), NonFinite> {
        self.path.pop();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::check;
    use galaxy_contract::geom::Vec2;

    #[test]
    fn finds_non_finite_numbers() {
        assert!(check(&Vec2::new(1.0, 2.0)).is_ok());
        let err = check(&Vec2::new(1.0, f32::NAN)).unwrap_err();
        assert_eq!(err, "y is NaN");
        assert!(
            check(&vec![Some(f64::INFINITY)])
                .unwrap_err()
                .contains("inf")
        );
    }
}
