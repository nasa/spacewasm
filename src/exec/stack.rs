use crate::util::Vec;
use crate::Box;

pub struct Stack(Box<[u32]>);

impl Stack {
    pub fn new(stack_size: usize) -> Self {
        Stack(unsafe { Vec::new(stack_size as u32).unwrap().assume_init() }.into_boxed_slice())
    }

    #[inline]
    pub(crate) fn read_u32(&self, addr: usize) -> u32 {
        self.0[addr]
    }

    #[inline]
    pub(crate) fn read_f32(&self, addr: usize) -> f32 {
        f32::from_bits(self.read_u32(addr))
    }

    #[inline]
    pub(crate) fn read_u64(&self, addr: usize) -> u64 {
        let lo = self.0[addr];
        let hi = self.0[addr + 1];
        (lo as u64) | ((hi as u64) << 32)
    }

    #[inline]
    pub(crate) fn read_f64(&self, addr: usize) -> f64 {
        f64::from_bits(self.read_u64(addr))
    }

    #[inline]
    pub(crate) fn write_u32(&mut self, addr: usize, value: u32) {
        self.0[addr] = value;
    }

    #[inline]
    pub(crate) fn write_f32(&mut self, addr: usize, value: f32) {
        self.write_u32(addr, value.to_bits());
    }

    #[inline]
    pub(crate) fn write_u64(&mut self, addr: usize, value: u64) {
        self.0[addr] = value as u32;
        self.0[addr + 1] = (value >> 32) as u32;
    }

    #[inline]
    pub(crate) fn write_f64(&mut self, addr: usize, value: f64) {
        self.write_u64(addr, value.to_bits());
    }
}
