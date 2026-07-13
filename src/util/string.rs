use crate::util::Vec;
use crate::{Allocator, GlobalAllocator, ValidationError};
use core::fmt;
use core::fmt::Formatter;
use core::ops::Deref;

#[derive(Clone)]
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

impl<A: Allocator> fmt::Display for String<A> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str(self)
    }
}

impl<A: Allocator> fmt::Debug for String<A> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str(self)
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

impl<A: Allocator> AsRef<str> for String<A> {
    #[inline]
    fn as_ref(&self) -> &str {
        self
    }
}

macro_rules! impl_eq {
    ($lhs:ty, $rhs: ty) => {
        impl PartialEq<$rhs> for $lhs {
            #[inline]
            fn eq(&self, other: &$rhs) -> bool {
                PartialEq::eq(&self[..], &other[..])
            }
        }

        impl PartialEq<$lhs> for $rhs {
            #[inline]
            fn eq(&self, other: &$lhs) -> bool {
                PartialEq::eq(&self[..], &other[..])
            }
        }
    };
}

impl_eq! { String, str }
impl_eq! { String, &str }

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
    extern crate std;

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

    #[test]
    fn test_from_vec_invalid() {
        let mut vec = Vec::new(3).unwrap();
        vec.push(0xFF);
        vec.push(0xFE);
        vec.push(0xFD);

        let result = String::try_from(vec);
        assert!(matches!(result, Err(ValidationError::MalformedUtf8)));
    }

    #[test]
    fn test_display() {
        let s = String::try_from("display me").unwrap();
        assert_eq!(std::format!("{}", s), "display me");
    }

    #[test]
    fn test_debug() {
        let s = String::try_from("debug me").unwrap();
        assert_eq!(std::format!("{:?}", s), "debug me");
    }

    #[test]
    fn test_as_ref() {
        let s = String::try_from("as ref").unwrap();
        let r: &str = s.as_ref();
        assert_eq!(r, "as ref");
    }

    #[test]
    fn test_clone() {
        let s = String::try_from("clone me").unwrap();
        let c = s.clone();
        assert_eq!(&*c, "clone me");
        assert_eq!(&*s, &*c);
    }

    #[test]
    fn test_eq_str() {
        let s = String::try_from("match").unwrap();

        // Exercise both directions of `impl_eq! { String, str }`.
        assert_eq!(s, *"match");
        assert_eq!(*"match", s);
        assert_ne!(s, *"nope");

        // And both directions of `impl_eq! { String, &str }`.
        assert_eq!(s, "match");
        assert_eq!("match", s);
        assert_ne!(s, "nope");
    }
}
