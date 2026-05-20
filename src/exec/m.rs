#[cfg(feature = "std-math")]
extern crate std;

#[cfg(not(feature = "std-math"))]
pub fn ceilf(f: f32) -> f32 {
    libm::ceilf(f)
}

#[cfg(feature = "std-math")]
pub fn ceilf(f: f32) -> f32 {
    std::primitive::f32::ceil(f)
}

#[cfg(not(feature = "std-math"))]
pub fn floorf(f: f32) -> f32 {
    libm::floorf(f)
}

#[cfg(feature = "std-math")]
pub fn floorf(f: f32) -> f32 {
    std::primitive::f32::floor(f)
}

#[cfg(not(feature = "std-math"))]
pub fn truncf(f: f32) -> f32 {
    libm::truncf(f)
}

#[cfg(feature = "std-math")]
pub fn truncf(f: f32) -> f32 {
    std::primitive::f32::trunc(f)
}

#[cfg(not(feature = "std-math"))]
pub fn roundf(f: f32) -> f32 {
    libm::roundf(f)
}

#[cfg(feature = "std-math")]
pub fn roundf(f: f32) -> f32 {
    std::primitive::f32::round(f)
}

#[cfg(not(feature = "std-math"))]
pub fn sqrtf(f: f32) -> f32 {
    libm::sqrtf(f)
}

#[cfg(feature = "std-math")]
pub fn sqrtf(f: f32) -> f32 {
    std::primitive::f32::sqrt(f)
}

#[cfg(not(feature = "std-math"))]
pub fn fminf(a: f32, b: f32) -> f32 {
    libm::fminf(a, b)
}

#[cfg(feature = "std-math")]
pub fn fminf(a: f32, b: f32) -> f32 {
    std::primitive::f32::min(a, b)
}

#[cfg(not(feature = "std-math"))]
pub fn fmaxf(a: f32, b: f32) -> f32 {
    libm::fmaxf(a, b)
}

#[cfg(feature = "std-math")]
pub fn fmaxf(a: f32, b: f32) -> f32 {
    std::primitive::f32::max(a, b)
}

#[cfg(not(feature = "std-math"))]
pub fn copysignf(a: f32, b: f32) -> f32 {
    libm::copysignf(a, b)
}

#[cfg(feature = "std-math")]
pub fn copysignf(a: f32, b: f32) -> f32 {
    std::primitive::f32::copysign(a, b)
}

#[cfg(not(feature = "std-math"))]
pub fn ceil(f: f64) -> f64 {
    libm::ceil(f)
}

#[cfg(feature = "std-math")]
pub fn ceil(f: f64) -> f64 {
    std::primitive::f64::ceil(f)
}

#[cfg(not(feature = "std-math"))]
pub fn floor(f: f64) -> f64 {
    libm::floor(f)
}

#[cfg(feature = "std-math")]
pub fn floor(f: f64) -> f64 {
    std::primitive::f64::floor(f)
}

#[cfg(not(feature = "std-math"))]
pub fn trunc(f: f64) -> f64 {
    libm::trunc(f)
}

#[cfg(feature = "std-math")]
pub fn trunc(f: f64) -> f64 {
    std::primitive::f64::trunc(f)
}

#[cfg(not(feature = "std-math"))]
pub fn round(f: f64) -> f64 {
    libm::round(f)
}

#[cfg(feature = "std-math")]
pub fn round(f: f64) -> f64 {
    std::primitive::f64::round(f)
}

#[cfg(not(feature = "std-math"))]
pub fn sqrt(f: f64) -> f64 {
    libm::sqrt(f)
}

#[cfg(feature = "std-math")]
pub fn sqrt(f: f64) -> f64 {
    std::primitive::f64::sqrt(f)
}

#[cfg(not(feature = "std-math"))]
pub fn fmin(a: f64, b: f64) -> f64 {
    libm::fmin(a, b)
}

#[cfg(feature = "std-math")]
pub fn fmin(a: f64, b: f64) -> f64 {
    std::primitive::f64::min(a, b)
}

#[cfg(not(feature = "std-math"))]
pub fn fmax(a: f64, b: f64) -> f64 {
    libm::fmax(a, b)
}

#[cfg(feature = "std-math")]
pub fn fmax(a: f64, b: f64) -> f64 {
    std::primitive::f64::max(a, b)
}

#[cfg(not(feature = "std-math"))]
pub fn copysign(a: f64, b: f64) -> f64 {
    libm::copysign(a, b)
}

#[cfg(feature = "std-math")]
pub fn copysign(a: f64, b: f64) -> f64 {
    std::primitive::f64::copysign(a, b)
}
