//! Native-stack guard for recursive execution.
//!
//! The executor walks the AST recursively, so shell-level recursion —
//! a function calling itself, `eval` evaluating itself, a dot script
//! sourcing itself — recurses on the native stack too. Unbounded, it
//! overflows the stack and the process aborts with SIGABRT/SIGSEGV,
//! skipping the EXIT trap. Instead of a fixed depth limit (a frame's size
//! differs by an order of magnitude between debug and release builds),
//! the recursion points check how much of the *actual* thread stack is
//! left and turn exhaustion into a shell error
//! ([`Executor::recursion_too_deep`]), so the reachable depth adapts to
//! the build and to `ulimit -s`.

use std::cell::Cell;

use super::Executor;
use crate::env::FlowControl;

/// Stack that must still be free at a recursion point for execution to
/// go one level deeper. It covers the deepest stretch between two
/// recursion points: a whole command's execution including its word
/// expansions, a nested parse of an `eval` string or a dot script, and a
/// trap action run at a command boundary. The worst bounded case is an
/// arithmetic expression at its nesting limit (`$(( ((…1…)) ))`, 256
/// levels): measured at ~13 KiB per level (~3.3 MiB) in a debug build
/// and well under 1 MiB in release, where an ordinary command needs only
/// a few KiB.
const STACK_RESERVE: usize = if cfg!(debug_assertions) {
    4 << 20
} else {
    1 << 20
};

/// Stack that must still be free at one level of a recursive descent
/// that has no fixed bound of its own fitting the stack: the parser and
/// lexer (an `eval` / `.` parse can start with only [`STACK_RESERVE`]
/// left, less than a maximal parse needs), arithmetic evaluation and the
/// expansion of nested `${name-word}` words. Past it the descent fails
/// with a shell error (syntax, arithmetic or expansion error) that
/// unwinds to a shallower frame before anything else runs, so it only has
/// to cover the frames between two checks plus building the error.
const DESCENT_RESERVE: usize = if cfg!(debug_assertions) {
    256 << 10
} else {
    64 << 10
};

thread_local! {
    /// `(lowest usable stack address, reserve, descent reserve)` of this
    /// thread, computed on first use (`(0, 0, 0)` = not yet computed). The
    /// reserves are [`STACK_RESERVE`] and [`DESCENT_RESERVE`], capped at a
    /// half and an eighth of the thread's stack so a small stack (a 2 MiB
    /// test thread) still runs shallow recursion.
    static STACK_LIMIT: Cell<(usize, usize, usize)> = const { Cell::new((0, 0, 0)) };
}

/// Approximate current stack pointer: the address of a local in a frame
/// that is never inlined. Assumes a downward-growing stack (every target
/// yosh supports).
#[inline(never)]
fn approx_stack_pointer() -> usize {
    let probe = 0u8;
    std::hint::black_box(&probe) as *const u8 as usize
}

/// `(lowest address, size)` of the current thread's stack, from the
/// platform's thread API.
#[cfg(target_vendor = "apple")]
fn query_stack() -> Option<(usize, usize)> {
    // SAFETY: plain queries on the calling thread's own pthread handle.
    unsafe {
        let thread = libc::pthread_self();
        let top = libc::pthread_get_stackaddr_np(thread) as usize;
        let size = libc::pthread_get_stacksize_np(thread);
        (top > size && size != 0).then(|| (top - size, size))
    }
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn query_stack() -> Option<(usize, usize)> {
    // SAFETY: `attr` is initialized by pthread_getattr_np before use and
    // destroyed afterwards; the out-pointers are valid locals.
    unsafe {
        let mut attr: libc::pthread_attr_t = std::mem::zeroed();
        if libc::pthread_getattr_np(libc::pthread_self(), &mut attr) != 0 {
            return None;
        }
        let mut addr: *mut libc::c_void = std::ptr::null_mut();
        let mut size: libc::size_t = 0;
        let rc = libc::pthread_attr_getstack(&attr, &mut addr, &mut size);
        libc::pthread_attr_destroy(&mut attr);
        (rc == 0 && !addr.is_null() && size != 0).then_some((addr as usize, size))
    }
}

#[cfg(not(any(target_vendor = "apple", target_os = "linux", target_os = "android")))]
fn query_stack() -> Option<(usize, usize)> {
    None
}

/// Fallback when the platform cannot report the stack bounds: assume
/// the stack extends `RLIMIT_STACK` (at most 8 MiB, the common main
/// thread size) below the current frame, which is taken while the
/// recursion is still shallow.
fn estimate_stack() -> (usize, usize) {
    let mut lim = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    // SAFETY: getrlimit only writes the provided struct.
    let size = if unsafe { libc::getrlimit(libc::RLIMIT_STACK, &mut lim) } == 0
        && lim.rlim_cur != libc::RLIM_INFINITY
    {
        (lim.rlim_cur as usize).min(8 << 20)
    } else {
        8 << 20
    };
    (approx_stack_pointer().saturating_sub(size), size)
}

fn stack_limit() -> (usize, usize, usize) {
    STACK_LIMIT.with(|c| {
        let mut limit = c.get();
        if limit.0 == 0 {
            let (low, size) = query_stack().unwrap_or_else(estimate_stack);
            limit = (
                low.max(1),
                STACK_RESERVE.min(size / 2),
                DESCENT_RESERVE.min(size / 8),
            );
            c.set(limit);
        }
        limit
    })
}

/// Bytes of native stack left below the current frame.
#[cfg(test)]
fn stack_remaining() -> usize {
    approx_stack_pointer().saturating_sub(stack_limit().0)
}

/// True when going one recursion level deeper could overflow the stack.
#[cfg(test)]
fn stack_nearly_exhausted() -> bool {
    stack_below_reserve(false)
}

/// True when less than the recursion reserve — or half of it for
/// `in_trap_action` (see [`Executor::recursion_too_deep`]) — is left.
fn stack_below_reserve(in_trap_action: bool) -> bool {
    let (low, reserve, _) = stack_limit();
    let reserve = if in_trap_action { reserve / 2 } else { reserve };
    approx_stack_pointer().saturating_sub(low) < reserve
}

/// True when one more level of a recursive descent (parsing, arithmetic,
/// nested word expansion) could overflow the stack (see
/// [`DESCENT_RESERVE`]). These levels check the absolute stack left, not
/// only a depth counted from where the descent began, because a nested
/// parse or expansion can begin as little as [`STACK_RESERVE`] from the
/// end of the stack.
pub(crate) fn descent_stack_exhausted() -> bool {
    let (low, _, reserve) = stack_limit();
    approx_stack_pointer().saturating_sub(low) < reserve
}

impl Executor {
    /// Check the stack before entering one more level of shell-level
    /// recursion (`what` names it for the diagnostic: a function name,
    /// `eval`, `.`). When the stack is nearly exhausted, report the error
    /// and raise it as a fatal error that unwinds to the top level like a
    /// fatal expansion error: a non-interactive shell then exits with
    /// status 1 after running its EXIT trap, an interactive shell abandons
    /// the command line. Returns true when the caller must not recurse.
    ///
    /// Inside a trap action the guard only demands half of the reserve
    /// (see `eval_trap_text`): the action must still run where the code
    /// it interrupted stood just under the full reserve.
    pub(crate) fn recursion_too_deep(&mut self, what: &str) -> bool {
        if !stack_below_reserve(self.trap_action_depth > 0) {
            return false;
        }
        crate::byteenc::write_stderr_decoded_line(&format!(
            "yosh: {}: maximum nesting level exceeded",
            what
        ));
        self.env.exec.last_exit_status = 1;
        self.env.exec.flow_control = Some(FlowControl::ExpansionError);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stack_bounds_are_plausible() {
        let remaining = stack_remaining();
        // Test threads have at least a 2 MiB stack and this frame is shallow.
        assert!(remaining > stack_limit().1, "remaining = {remaining}");
        assert!(!stack_nearly_exhausted());
        assert!(!descent_stack_exhausted());
        assert!(remaining < 1 << 30, "remaining = {remaining}");
    }

    #[test]
    fn remaining_shrinks_with_depth() {
        #[inline(never)]
        fn deeper(n: usize) -> usize {
            let pad = std::hint::black_box([0u8; 1024]);
            if n == 0 {
                stack_remaining() + pad[0] as usize
            } else {
                deeper(n - 1) + pad[1] as usize
            }
        }
        let shallow = stack_remaining();
        let deep = deeper(16);
        assert!(deep + 16 * 1024 <= shallow, "shallow={shallow} deep={deep}");
    }
}
