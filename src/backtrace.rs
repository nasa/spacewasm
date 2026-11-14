use crate::{CallFrame, Engine, JumpTarget, Stack};

/// IR program counters of the call frames on the stack, innermost first. Created
/// by [`Engine::backtrace`].
///
/// Element 0 is the current instruction; each later element is the `call` /
/// `call_indirect` instruction in the caller, not a return address. The walk
/// never allocates and stops at a malformed frame instead of panicking.
pub struct Backtrace<'a> {
    stack: &'a Stack,
    /// The element to yield next.
    next: Option<JumpTarget>,
    /// Frame pointer of the function `next` belongs to, or `None` once that
    /// frame's caller is known to be unreachable.
    fp: Option<u32>,
}

impl<'a> Backtrace<'a> {
    pub(crate) fn new(stack: &'a Stack, pc: JumpTarget, fp: u32) -> Backtrace<'a> {
        Backtrace {
            stack,
            next: Some(pc),
            fp: Some(fp),
        }
    }
}

impl Iterator for Backtrace<'_> {
    type Item = JumpTarget;

    fn next(&mut self) -> Option<JumpTarget> {
        let pc = self.next.take()?;

        // A frame is `[CallFrame bits][caller's call PC]` at `fp`, see `call_impl`.
        if let Some(fp) = self.fp.take() {
            let fp = fp as usize;
            if fp < self.stack.len() && self.stack.len() - fp >= 2 {
                let caller_pc = JumpTarget(self.stack.read_u32(fp + 1));

                // The outermost frame stores SENTINEL in place of a caller.
                if caller_pc != JumpTarget::SENTINEL {
                    self.next = Some(caller_pc);

                    // The caller's frame must lie strictly below this one,
                    // which is also what guarantees the walk ends.
                    let frame_length = CallFrame::from_bits(self.stack.read_u32(fp)).frame_length;
                    if frame_length != 0 && frame_length as usize <= fp {
                        self.fp = Some((fp - frame_length as usize) as u32);
                    }
                }
            }
        }

        Some(pc)
    }
}

impl core::iter::FusedIterator for Backtrace<'_> {}

impl Engine {
    /// The call stack between [`run`](crate::InterpreterRunner::run) calls.
    /// Element 0 is the next instruction to run, the host `call` while paused, or
    /// the faulting instruction after a trap (kept until the next
    /// [`Engine::invoke`] or [`Engine::reset`]).
    ///
    /// `None` if nothing has been invoked or the last call finished.
    pub fn backtrace(&self) -> Option<Backtrace<'_>> {
        if !self.is_idle() {
            return Some(Backtrace::new(&self.stack, self.pc, self.fp));
        }
        let pc = self.trap_pc?;
        Some(Backtrace::new(&self.stack, pc, self.trap_fp))
    }
}
