use crate::{CallFrame, Engine, JumpTarget, Stack};

/// Walks the call frames on the interpreter stack, yielding IR program counters
/// innermost first. Created by [`Engine::backtrace`].
///
/// - Element 0 is the PC of the innermost function: the instruction the engine
///   is at, which [`Engine::backtrace`] describes.
/// - Every later element is the PC of the `call` / `call_indirect` instruction
///   in the next-outer function that made the call. It is the address of the
///   call instruction itself, not its return address.
///
/// Every element is the start of an instruction, so a symbol lookup needs no
/// "return address minus one" adjustment.
///
/// The walk ends at the outermost frame, the one started by
/// [`Engine::invoke`]. It does not allocate, and it reads nothing outside the
/// stack: if a frame record is out of bounds or does not point to a strictly
/// lower frame, the walk stops there instead of panicking. That bounds it at
/// one step per stack word.
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
    /// The call stack as the engine stands between
    /// [`run`](crate::InterpreterRunner::run) calls, innermost first. See
    /// [`Backtrace`] for what each element means.
    ///
    /// Element 0 is the instruction the engine is at:
    /// - out of fuel, or invoked and not yet run, the next instruction to
    ///   execute;
    /// - paused, the `call` / `call_indirect` instruction that called the
    ///   pausing host function. Only [`Engine::resume`] moves past it;
    /// - after a trap, the faulting instruction. The trap resets the engine to
    ///   idle but leaves the stack memory as it was, so the frames are still
    ///   there until the next [`Engine::invoke`] or [`Engine::reset`].
    ///
    /// A host function that wants its call stack pauses, and the embedder reads
    /// it before resuming.
    ///
    /// Returns `None` when there is nothing to report: no call has been
    /// invoked, or the last one finished.
    pub fn backtrace(&self) -> Option<Backtrace<'_>> {
        if !self.is_idle() {
            return Some(Backtrace::new(&self.stack, self.pc, self.fp));
        }
        let pc = self.trap_pc?;
        Some(Backtrace::new(&self.stack, pc, self.trap_fp))
    }
}
