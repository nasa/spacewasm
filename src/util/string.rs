use crate::util::Vec;
use crate::{Allocator, GlobalAllocator, ValidationError};
use core::ops::Deref;

pub struct String<A: Allocator = GlobalAllocator>(Vec<u8, A>);

impl TryFrom<&[u8]> for String<GlobalAllocator> {
    type Error = ValidationError;

    fn try_from(value: &[u8]) -> Result<String, ValidationError> {
        match core::str::from_utf8(value) {
            Ok(s) => s.try_into(),
            Err(_) => Err(ValidationError::MalformedUtf8),
        }
    }
}

impl TryFrom<&str> for String<GlobalAllocator> {
    type Error = ValidationError;

    fn try_from(value: &str) -> Result<Self, ValidationError> {
        let mut v = Vec::new(value.len() as u32)?;
        for byte in value.as_bytes() {
            v.push(*byte);
        }
        Ok(String(v))
    }
}

impl<A: Allocator> TryFrom<Vec<u8, A>> for String<A> {
    type Error = ValidationError;

    fn try_from(value: Vec<u8, A>) -> Result<Self, Self::Error> {
        // Validate the integrity of the string
        match core::str::from_utf8(&value) {
            Ok(_) => Ok(String(value)),
            Err(_) => Err(ValidationError::MalformedUtf8),
        }
    }
}

impl<A: Allocator> Deref for String<A> {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        unsafe { core::str::from_utf8_unchecked(&self.0) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_from_str() {
        let s = String::try_from("hello").unwrap();
        assert_eq!(&*s, "hello");
    }

    #[test]
    fn test_from_bytes_valid() {
        let bytes = b"world";
        let s = String::try_from(&bytes[..]).unwrap();
        assert_eq!(&*s, "world");
    }

    #[test]
    fn test_from_bytes_invalid() {
        let invalid_bytes = &[0xFF, 0xFE, 0xFD];
        let result = String::try_from(&invalid_bytes[..]);
        assert!(matches!(result, Err(ValidationError::MalformedUtf8)));
    }

    #[test]
    fn test_from_vec() {
        let mut vec = Vec::new(5).unwrap();
        vec.push(b'h');
        vec.push(b'e');
        vec.push(b'l');
        vec.push(b'l');
        vec.push(b'o');

        let s = String::try_from(vec).unwrap();
        assert_eq!(&*s, "hello");
    }
}
