# macOS owner identity for reservation recovery

This experimental change covers owner identity and recovery of an unstarted reservation; it does not contain or prove the exit of a process tree. Existing reservations without a boot scope, inaccessible identities, changed boot scopes, ambiguous spawn intent and unconfirmed descendant exit remain unknown.

On macOS the implementation obtains the owner's PID and `PROC_PIDTBSDINFO` creation time using `proc_pidinfo`, and requests the boot session UUID through `kern.bootsessionuuid`. A complete, nonzero UUID is stored separately as `owner_scope`; recovery rechecks it and the owner inside the existing SQLite write transaction. A missing PID is treated as exited only when both `proc_pidinfo` and `kill(pid, 0)` return `ESRCH`; other errors are unknown. The existing `exit_confirmed` condition is unchanged and must only be set by a separately verified process-tree boundary.

The local Apple-origin `libproc.h` and `sys/proc_info.h` declare the process API and fields; the Apple `IOPM.h` reference supplied for this task describes the boot session UUID. The correspondence and availability of `kern.bootsessionuuid`, process visibility and error behavior have not been verified on a Mac. Cross-checks compile code only: before integration, test real Apple Silicon and Intel systems, permission and sandbox denial, live and exited owners, PID reuse, sleep, reboot, missing UUID, legacy records and concurrent recovery. Never infer descendant exit from these identities.

This older worktree does not contain all of the newer `younchan` Linux scope and reservation protections. Carry the specific macOS changes into the newer implementation without replacing its files wholesale.
