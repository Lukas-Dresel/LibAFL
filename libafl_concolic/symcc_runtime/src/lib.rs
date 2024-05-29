//! # `SymCC` Runtime Bindings
//! This crate contains bindings to the [`SymCC`](https://github.com/eurecom-s3/symcc) [runtime interface](https://github.com/eurecom-s3/symcc/blob/master/runtime/RuntimeCommon.h) to be used from Rust.
//! A `SymCC` runtime can be used with either `SymCC` or [`SymQEMU`](https://github.com/eurecom-s3/symqemu) to trace the execution of a target program.
//!
//! ## How to use
//! On a high level, users of this crate can implement the [`Runtime`] trait and export the runtime interface as a `cdylib` using the [`export_runtime`] macro.
//! On a technical level, a `SymCC` runtime is a dynamic library (/shared object) that exposes a set of symbols that the instrumentation layer of `SymCC` calls into during execution of the target.
//! Therefore, to create a runtime, a separate crate for the runtime is required, because this is the only way to create a separate dynamic library using cargo.
//!
//! ## Goodies
//! To facilitate common use cases, this crate also contains some pre-built functionality in the form of a [`tracing::TracingRuntime`] that traces the execution to a shared memory region.
//! It also contains a separate abstraction to easily filter the expressions that make up such a trace in the [`filter`] module.
//! For example, it contains a [`filter::NoFloat`] filter that concretizes all floating point operations in the trace, because those are usually more difficult to handle than discrete constraints.
//!
//! ## Crate setup
//! Your runtime crate should have the following keys set in its `Cargo.toml`:
//! ```toml
//! [profile.release]
//! # this is somewhat important to ensure the runtime does not unwind into the target program.
//! panic = "abort"
//! [profile.debug]
//! panic = "abort"
//!
//! [lib]
//! # this is required for the output to be a shared object (.so file)
//! crate-type   = ["cdylib"]
//! # SymCC and SymQEMU expect to runtime file to be called `libSymRuntime.so`. Setting the name to `SymRuntime` achieves this.
//! name = "SymRuntime"
//! ```
#![allow(clippy::module_name_repetitions, clippy::missing_panics_doc)]
pub mod filter;
pub mod tracing;
pub mod mem_model;

// The following exports are used by the `export_runtime` macro. They are therefore exported, but hidden from docs, as they are not supposed to be used directly by the user.
#[doc(hidden)]
#[cfg(target_os = "linux")]
pub mod cpp_runtime {
    #![allow(non_upper_case_globals)]
    #![allow(non_camel_case_types)]
    #![allow(non_snake_case)]
    include!(concat!(env!("OUT_DIR"), "/bindings.rs"));
}
pub mod cpp_interface {
    #![allow(non_upper_case_globals)]
    #![allow(non_camel_case_types)]
    #![allow(non_snake_case)]
    include!(concat!(env!("OUT_DIR"), "/interface.rs"));
}

pub fn get_symbolic_exprs_for_memory(addr: usize, length: usize) -> Vec<Option<RSymExpr>> {
    // allocate the output vector of desired length
    let mut sym_exprs = (0..length).map(|_| None).collect::<Vec<_>>();

    unsafe {
        cpp_interface::_sym_get_symbolic_exprs_for_memory(
            sym_exprs.as_mut_ptr() as *mut usize,
            addr as *mut c_void,
            length,
        );
    }
    // println!("sym_exprs: {:x?} bytes @ {:x?} => {:?}", length, addr, sym_exprs);

    sym_exprs
}

use std::os::raw::c_void;

#[doc(hidden)]
pub use ctor::ctor;
use libafl::observers::concolic;
#[doc(hidden)]
pub use libc::atexit;
#[doc(hidden)]
pub use unchecked_unwrap;

#[doc(hidden)]
#[cfg(target_os = "linux")]
#[macro_export]
macro_rules! export_c_symbol {
    (pub fn $name:ident($( $arg:ident : $type:ty ),*$(,)?) -> $ret:ty) => {
        use $crate::cpp_runtime::*;
        #[no_mangle]
        pub unsafe extern "C" fn $name($( $arg : $type),*) -> $ret {
            $crate::cpp_runtime::$name($( $arg ),*)
        }
    };
    (pub fn $name:ident($( $arg:ident : $type:ty ),* $(,)?)) => {
        $crate::export_c_symbol!(pub fn $name($( $arg : $type),*) -> ());
    }
}

#[cfg(target_os = "linux")]
include!(concat!(env!("OUT_DIR"), "/cpp_exports_macro.rs"));

include!(concat!(env!("OUT_DIR"), "/rust_exports_macro.rs"));

macro_rules! rust_runtime_function_declaration {
    (pub fn expression_unreachable(expressions: *mut RSymExpr, num_elements: usize), $c_name:ident;) => {
        fn expression_unreachable(&mut self, exprs: &[RSymExpr]);
    };

    (pub fn backend_read_memory(
        addr_expr: RSymExpr,
        concolic_read_value: RSymExpr,
        addr: *mut u8,
        length: usize,
        little_endian: bool,
    ) -> RSymExpr, $c_name:ident;) => {
        #[allow(clippy::default_trait_access)]
        fn backend_read_memory(
            &mut self,
            addr_expr: Option<RSymExpr>,
            concolic_read_value: Option<RSymExpr>,
            addr: *mut u8,
            length: usize,
            little_endian: bool,
        ) -> Option<RSymExpr>;
    };

    (pub fn backend_write_memory(
        symbolic_addr_expr: RSymExpr,
        written_expr: RSymExpr,
        concrete_addr: *mut u8,
        concrete_length: usize,
        little_endian: bool,
    ), $c_name:ident;) => {
        fn backend_write_memory(
            &mut self,
            symbolic_addr_expr: Option<RSymExpr>,
            written_expr: Option<RSymExpr>,
            concrete_addr: *mut u8,
            concrete_length: usize,
            little_endian: bool,
        );
    };

    (pub fn backend_memcpy(
        sym_dest: RSymExpr,
        sym_src: RSymExpr,
        sym_len: RSymExpr,
        dest: *mut u8,
        src: *const u8,
        length: usize,
    ), $c_name:ident;) => {
        fn backend_memcpy(
            &mut self,
            sym_dest: Option<RSymExpr>,
            sym_src: Option<RSymExpr>,
            sym_len: Option<RSymExpr>,
            dest: *mut u8,
            src: *const u8,
            length: usize,
        );
    };
    (pub fn backend_memmove(
        sym_dest: RSymExpr,
        sym_src: RSymExpr,
        sym_len: RSymExpr,
        dest: *mut u8,
        src: *const u8,
        length: usize,
    ), $c_name:ident;) => {
        fn backend_memmove(
            &mut self,
            sym_dest: Option<RSymExpr>,
            sym_src: Option<RSymExpr>,
            sym_len: Option<RSymExpr>,
            dest: *mut u8,
            src: *const u8,
            length: usize,
        );
    };
    (pub fn backend_memset(
        sym_dest: RSymExpr,
        sym_val: RSymExpr,
        sym_len: RSymExpr,
        memory: *mut u8,
        value: ::std::os::raw::c_int,
        length: usize,
    ), $c_name:ident;) => {
        fn backend_memset(
            &mut self,
            sym_dest: Option<RSymExpr>,
            sym_val: Option<RSymExpr>,
            sym_len: Option<RSymExpr>,
            memory: *mut u8,
            value: ::std::os::raw::c_int,
            length: usize,
        );
    };

    (pub fn $name:ident($( $arg:ident : $type:ty ),*$(,)?)$( -> $ret:ty)?, $c_name:ident;) => {
        // compile_error!(stringify!($name));
        fn $name(&mut self, $( $arg : $type),*)$( -> Option<$ret>)?;
    };
}

/// Values of this type identify an expression. They can be thought of as references to expressions.
///
/// All values of this type are produced by the `build_*` methods on [`Runtime`] and subsequently consumed by the runtime.
/// Therefore, how these values are interpreted is entirely up to the runtime.
/// They are pointer-sized and are required to be non-zero.
/// Therefore this type resolves to [`core::num::NonZeroUsize`].
pub type RSymExpr = concolic::SymExprRef;

/// This trait encapsulates the full interface of a runtime.
/// The individual methods of this trait are not documented, but follow a simple rules:
///
/// 1. methods starting with `build_`  or end in `_helper` create new expressions in the trace
/// 2. `Runtime::get_input_byte` creates variable expressions
/// 3. methods starting with `notify_` trace give an indication as to where in the code the execution is at (using random, but stable identifiers)
/// 4. `Runtime::push_path_constraint` creates a root expression (no other expressions can reference this) and marks a path constraint in the execution of the target
/// 5. `Runtime::expression_unreachable` is called regularly to with [`RSymExpr`]'s that won't be referenced in future calls to the runtime (because they are not reachable anymore)
///
/// All methods that create new expressions return `Option<RSymExpr>`. Returning `Option::None` will concretize the result of the expression.
/// For example, returning `None` from `Runtime::build_udiv` will concretize the result of all unsidned integer division operations.
/// Filtering expressions like this is also facilitated by [`filter::Filter`].
///
/// ## Drop
/// The code generated from `export_runtime` will attempt to drop your runtime.
/// In the context of fuzzing it is expected that the process may crash and in this case, the runtime will not be dropped.
/// Therefore, any runtime should make sure to handle this case properly (for example by flushing buffers regularly).
pub trait Runtime {
    invoke_macro_with_rust_runtime_exports!(rust_runtime_function_declaration;);
}

#[doc(hidden)]
#[macro_export]
macro_rules! make_symexpr_optional {
    (RSymExpr) => {Option<RSymExpr>};
    ($($type:tt)+) => {$($type)+};
}

#[doc(hidden)]
#[macro_export]
macro_rules! unwrap_option {
    ($param_name:ident: RSymExpr) => {
        $param_name?
    };
    ($param_name:ident: $($type:tt)+) => {
        $param_name
    };
}

/// Creates an exported extern C function for the given runtime function declaration, forwarding to the runtime as obtained by `$rt_cb` (which should be `fn (fn (&mut impl Runtime))`).
#[doc(hidden)]
#[macro_export]
macro_rules! export_rust_runtime_fn {
    // special case for expression_unreachable, because we need to be convert pointer+length to slice
    (pub fn expression_unreachable(expressions: *mut RSymExpr, num_elements: usize), $c_name:ident; $rt_cb:path) => {
        #[allow(clippy::missing_safety_doc)]
        #[no_mangle]
        pub unsafe extern "C" fn _rsym_expression_unreachable(expressions: *mut RSymExpr, num_elements: usize) {
            let slice = core::slice::from_raw_parts(expressions, num_elements);
            $rt_cb(|rt| {
                rt.expression_unreachable(slice);
            })
        }
    };
    // special case for push_path_constraint, we are not returning a new expression while taking an expression as argument
    (pub fn push_path_constraint(constraint: RSymExpr, taken: bool, site_id: usize), $c_name:ident; $rt_cb:path) => {
        #[allow(clippy::missing_safety_doc)]
        #[no_mangle]
        pub unsafe extern "C" fn _rsym_push_path_constraint(constraint: Option<RSymExpr>, taken: bool, site_id: usize) {
            if let Some(constraint) = constraint {
                $rt_cb(|rt| {
                    rt.push_path_constraint(constraint, taken, site_id);
                })
            }
        }
    };

    (pub fn concretize_pointer(value: RSymExpr, ptr: usize, site_id: usize), $c_name:ident; $rt_cb:path) => {
        #[allow(clippy::missing_safety_doc)]
        #[no_mangle]
        pub unsafe extern "C" fn _rsym_concretize_pointer(expr: Option<RSymExpr>, ptr: usize, site_id: usize) {
            if let Some(expr) = expr {
                $rt_cb(|rt| {
                    rt.concretize_pointer(expr, ptr, site_id);
                })
            }
        }
    };
    (pub fn concretize_size(value: RSymExpr, sz: usize, site_id: usize), $c_name:ident; $rt_cb:path) => {
        #[allow(clippy::missing_safety_doc)]
        #[no_mangle]
        pub unsafe extern "C" fn _rsym_concretize_size(expr: Option<RSymExpr>, value: usize, site_id: usize) {
            if let Some(expr) = expr {
                $rt_cb(|rt| {
                    rt.concretize_size(expr, value, site_id);
                })
            }
        }
    };
    (pub fn notify_param_expr(index: u8, expr: RSymExpr), $c_name:ident; $rt_cb:path) => {
        #[allow(clippy::missing_safety_doc)]
        #[no_mangle]
        pub unsafe extern "C" fn _rsym_notify_param_expr(index: u8, expr: Option<RSymExpr>) {
            $rt_cb(|rt| {
                if let Some(expr) = expr {
                    rt.notify_param_expr(index, expr);
                }
            })
        }
    };
    (pub fn notify_ret_expr(expr: RSymExpr), $c_name:ident; $rt_cb:path) => {
        #[allow(clippy::missing_safety_doc)]
        #[no_mangle]
        pub unsafe extern "C" fn _rsym_notify_ret_expr(expr: Option<RSymExpr>) {
            $rt_cb(|rt| {
                if let Some(expr) = expr {
                    rt.notify_ret_expr(expr);
                }
            })
        }
    };

    (pub fn backend_read_memory(
        addr_expr: RSymExpr,
        concolic_read_value: RSymExpr,
        addr: *mut u8,
        length: usize,
        little_endian: bool,
    ) -> RSymExpr, $c_name:ident; $rt_cb:path) => {
        #[allow(clippy::missing_safety_doc)]
        #[no_mangle]
        pub unsafe extern "C" fn _rsym_backend_read_memory(
            addr_expr: Option<RSymExpr>,
            concolic_read_value: Option<RSymExpr>,
            addr: *mut u8,
            length: usize,
            little_endian: bool,
        ) -> Option<RSymExpr> {
            // std::fs::write("/tmp/backend_read_memory", format!("addr_expr: {:?}, concolic_read_value: {:?}, addr: {:p}, length: {}, little_endian: {}", addr_expr, concolic_read_value, addr, length, little_endian)).unwrap();

            return $rt_cb(|rt| {
                rt.backend_read_memory(addr_expr, concolic_read_value, addr, length, little_endian)
            });
        }
    };

    (pub fn backend_write_memory(
        symbolic_addr_expr: RSymExpr,
        written_expr: RSymExpr,
        concrete_addr: *mut u8,
        concrete_length: usize,
        little_endian: bool,
    ), $c_name:ident; $rt_cb:path) => {
        #[allow(clippy::missing_safety_doc)]
        #[no_mangle]
        pub unsafe extern "C" fn _rsym_backend_write_memory(
            symbolic_addr_expr: Option<RSymExpr>,
            written_expr: Option<RSymExpr>,
            concrete_addr: *mut u8,
            concrete_length: usize,
            little_endian: bool,
        ) {
            $rt_cb(|rt| {
                rt.backend_write_memory(symbolic_addr_expr, written_expr, concrete_addr, concrete_length, little_endian);
            })
        }
    };
    (pub fn backend_memcpy(
        sym_dest: RSymExpr,
        sym_src: RSymExpr,
        sym_len: RSymExpr,
        dest: *mut u8,
        src: *const u8,
        length: usize,
    ), $c_name:ident; $rt_cb:path) => {
        #[allow(clippy::missing_safety_doc)]
        #[no_mangle]
        pub unsafe extern "C" fn _rsym_backend_memcpy(
            sym_dest: Option<RSymExpr>,
            sym_src: Option<RSymExpr>,
            sym_len: Option<RSymExpr>,
            dest: *mut u8,
            src: *const u8,
            length: usize,
        ) {
            $rt_cb(|rt| {
                rt.backend_memcpy(sym_dest, sym_src, sym_len, dest, src, length);
            })
        }
    };
    (pub fn backend_memmove(
        sym_dest: RSymExpr,
        sym_src: RSymExpr,
        sym_len: RSymExpr,
        dest: *mut u8,
        src: *const u8,
        length: usize,
    ), $c_name:ident; $rt_cb:path) => {
        #[allow(clippy::missing_safety_doc)]
        #[no_mangle]
        pub unsafe extern "C" fn _rsym_backend_memmove(
            sym_dest: Option<RSymExpr>,
            sym_src: Option<RSymExpr>,
            sym_len: Option<RSymExpr>,
            dest: *mut u8,
            src: *const u8,
            length: usize,
        ) {
            $rt_cb(|rt| {
                rt.backend_memmove(sym_dest, sym_src, sym_len, dest, src, length);
            })
        }
    };
    (pub fn backend_memset(
        sym_dest: RSymExpr,
        sym_val: RSymExpr,
        sym_len: RSymExpr,
        memory: *mut u8,
        value: ::std::os::raw::c_int,
        length: usize,
    ), $c_name:ident; $rt_cb:path) => {
        #[allow(clippy::missing_safety_doc)]
        #[no_mangle]
        pub unsafe extern "C" fn _rsym_backend_memset(
            sym_dest: Option<RSymExpr>,
            sym_val: Option<RSymExpr>,
            sym_len: Option<RSymExpr>,
            memory: *mut u8,
            value: ::std::os::raw::c_int,
            length: usize,
        ) {
            $rt_cb(|rt| {
                rt.backend_memset(sym_dest, sym_val, sym_len, memory, value, length);
            })
        }
    };

    // all other methods are handled by this
    (pub fn $name:ident($( $arg:ident : $(::)?$($type:ident)::+ ),*$(,)?)$( -> $($ret:ident)::+)?, $c_name:ident; $rt_cb:path) => {
        // compile_error!(stringify!($c_name));
        #[allow(clippy::missing_safety_doc)]
        #[no_mangle]
        pub unsafe extern "C" fn $c_name( $($arg: $crate::make_symexpr_optional!($($type)::+),)* )$( -> $crate::make_symexpr_optional!($($ret)::+))? {
            $rt_cb(|rt| {
                $(let $arg = $crate::unwrap_option!($arg: $($type)::+);)*
                rt.$name($($arg,)*)
            })
        }
    };
}

/// implements the [`NopRuntime`] methods by returning [`Default::default`] from all methods.
macro_rules! impl_nop_runtime_fn {
    // special case for expression_unreachable, because it has a different signature in our runtime trait than in the c interface.
    (pub fn expression_unreachable(expressions: *mut RSymExpr, num_elements: usize), $c_name:ident;) => {
        #[allow(clippy::default_trait_access)]
        fn expression_unreachable(&mut self, _exprs: &[RSymExpr]) {std::default::Default::default()}
    };

    (pub fn backend_read_memory(
        addr_expr: RSymExpr,
        concolic_read_value: RSymExpr,
        addr: *mut u8,
        length: usize,
        little_endian: bool,
    ) -> RSymExpr, $c_name:ident; ) => {
        #[allow(clippy::default_trait_access)]
        #[allow(unused_variables)]
        fn backend_read_memory(
            &mut self,
            addr_expr: Option<RSymExpr>,
            concolic_read_value: Option<RSymExpr>,
            addr: *mut u8,
            length: usize,
            little_endian: bool,
        ) -> Option<RSymExpr> {
            // by default, we just return the concolic expression the runtime created for us.
            concolic_read_value
        }
    };

    (pub fn backend_write_memory(
        symbolic_addr_expr: RSymExpr,
        written_expr: RSymExpr,
        concrete_addr: *mut u8,
        concrete_length: usize,
        little_endian: bool,
    ), $c_name:ident; ) => {
        #[allow(unused_variables)]
        fn backend_write_memory(
            &mut self,
            symbolic_addr_expr: Option<RSymExpr>,
            written_expr: Option<RSymExpr>,
            concrete_addr: *mut u8,
            concrete_length: usize,
            little_endian: bool,
        )
        { }
    };

    (pub fn backend_memcpy(
        sym_dest: RSymExpr,
        sym_src: RSymExpr,
        sym_len: RSymExpr,
        dest: *mut u8,
        src: *const u8,
        length: usize,
    ), $c_name:ident;) => {
        #[allow(clippy::default_trait_access)]
        #[allow(unused_variables)]
        fn backend_memcpy(
            &mut self,
            sym_dest: Option<RSymExpr>,
            sym_src: Option<RSymExpr>,
            sym_len: Option<RSymExpr>,
            dest: *mut u8,
            src: *const u8,
            length: usize,
        )
        { }
    };
    (pub fn backend_memmove(
        sym_dest: RSymExpr,
        sym_src: RSymExpr,
        sym_len: RSymExpr,
        dest: *mut u8,
        src: *const u8,
        length: usize,
    ), $c_name:ident;) => {
        #[allow(clippy::default_trait_access)]
        #[allow(unused_variables)]
        fn backend_memmove(
            &mut self,
            sym_dest: Option<RSymExpr>,
            sym_src: Option<RSymExpr>,
            sym_len: Option<RSymExpr>,
            dest: *mut u8,
            src: *const u8,
            length: usize,
        )
        { }
    };
    (pub fn backend_memset(
        sym_dest: RSymExpr,
        sym_val: RSymExpr,
        sym_len: RSymExpr,
        memory: *mut u8,
        value: ::std::os::raw::c_int,
        length: usize,
    ), $c_name:ident;) => {
        #[allow(clippy::default_trait_access)]
        #[allow(unused_variables)]
        fn backend_memset(
            &mut self,
            sym_dest: Option<RSymExpr>,
            sym_val: Option<RSymExpr>,
            sym_len: Option<RSymExpr>,
            memory: *mut u8,
            value: ::std::os::raw::c_int,
            length: usize,
        )
        { }
    };

    (pub fn $name:ident($( $arg:ident : $type:ty ),*$(,)?)$( -> $ret:ty)?, $c_name:ident;) => {
        // error with the matched string
        // compile_error!(stringify!(fn $name(&mut self, $( $arg : $type),*)$( -> Option<$ret>)?));
        #[allow(clippy::default_trait_access)]
        fn $name(&mut self, $( _ : $type),*)$( -> Option<$ret>)? {std::default::Default::default()}
    };
}

/// This runtime does nothing and concretizes all expressions. Intended for testing purposes.
pub struct NopRuntime;

impl Runtime for NopRuntime {
    invoke_macro_with_rust_runtime_exports!(impl_nop_runtime_fn;);
}

/// This runtime can be constructed from an [`Option`] of a runtime, concretizing all expressions in the `None` case and forwarding expressions to the respective runtime in the `Some` case.
/// This is especially useful for parts of the processing pipeline that should be activated based on a runtime configuration, such as an environment variable.
pub struct OptionalRuntime<RT> {
    inner: Option<RT>,
}

impl<RT> OptionalRuntime<RT> {
    pub fn new(rt: Option<RT>) -> Self {
        Self { inner: rt }
    }

    pub fn into_inner(self) -> Option<RT> {
        self.inner
    }
}

macro_rules! rust_runtime_function_declaration {
    (pub fn expression_unreachable(expressions: *mut RSymExpr, num_elements: usize), $c_name:ident;) => {
        #[allow(clippy::default_trait_access)]
        fn expression_unreachable(&mut self, exprs: &[RSymExpr]) {
            if let Some(inner) = &mut self.inner {
                inner.expression_unreachable(exprs);
            }
        }
    };

    (pub fn notify_param_expr(index: u8, expr: RSymExpr), $c_name:ident;) => {
        #[allow(clippy::default_trait_access)]
        fn notify_param_expr(&mut self, index: u8, expr: RSymExpr) {
            if let Some(inner) = &mut self.inner {
                inner.notify_param_expr(index, expr);
            }
        }
    };

    (pub fn notify_ret_expr(expr: RSymExpr), $c_name:ident;) => {
        #[allow(clippy::default_trait_access)]
        fn notify_ret_expr(&mut self, expr: RSymExpr) {
            if let Some(inner) = &mut self.inner {
                inner.notify_ret_expr(expr);
            }
        }
    };
    (pub fn concretize_pointer(expr: RSymExpr, ptr: usize), $c_name:ident;) => {
        #[allow(clippy::default_trait_access)]
        fn concretize_pointer(&mut self, expr: RSymExpr, ptr: usize) {
            if let Some(inner) = &mut self.inner {
                inner.concretize_pointer(expr, ptr);
            }
        }
    };

    (pub fn backend_read_memory(
        addr_expr: RSymExpr,
        concolic_read_value: RSymExpr,
        addr: *mut u8,
        length: usize,
        little_endian: bool,
    ) -> RSymExpr, $c_name:ident; ) => {
        #[allow(clippy::default_trait_access)]
        fn backend_read_memory(
            &mut self,
            addr_expr: Option<RSymExpr>,
            concolic_read_value: Option<RSymExpr>,
            addr: *mut u8,
            length: usize,
            little_endian: bool,
        ) -> Option<RSymExpr> {
            // let sym_exprs = crate::cpp_runtime::_
            if let Some(inner) = &mut self.inner {
                return inner.backend_read_memory(addr_expr, concolic_read_value, addr, length, little_endian);
            }
            concolic_read_value
        }
    };

    (pub fn backend_write_memory(
        symbolic_addr_expr: RSymExpr,
        written_expr: RSymExpr,
        concrete_addr: *mut u8,
        concrete_length: usize,
        little_endian: bool,
    ), $c_name:ident;) => {
        fn backend_write_memory(
            &mut self,
            symbolic_addr_expr: Option<RSymExpr>,
            written_expr: Option<RSymExpr>,
            concrete_addr: *mut u8,
            concrete_length: usize,
            little_endian: bool,
        ) {
            if let Some(inner) = &mut self.inner {
                inner.backend_write_memory(symbolic_addr_expr, written_expr, concrete_addr, concrete_length, little_endian);
            }
        }
    };

    (pub fn backend_memcpy(
        sym_dest: RSymExpr,
        sym_src: RSymExpr,
        sym_len: RSymExpr,
        dest: *mut u8,
        src: *const u8,
        length: usize,
    ), $c_name:ident;) => {
        #[allow(clippy::default_trait_access)]
        fn backend_memcpy(
            &mut self,
            sym_dest: Option<RSymExpr>,
            sym_src: Option<RSymExpr>,
            sym_len: Option<RSymExpr>,
            dest: *mut u8,
            src: *const u8,
            length: usize,
        ) {
            if let Some(inner) = &mut self.inner {
                inner.backend_memcpy(sym_dest, sym_src, sym_len, dest, src, length);
            }
        }
    };
    (pub fn backend_memmove(
        sym_dest: RSymExpr,
        sym_src: RSymExpr,
        sym_len: RSymExpr,
        dest: *mut u8,
        src: *const u8,
        length: usize,
    ), $c_name:ident;) => {
        #[allow(clippy::default_trait_access)]
        fn backend_memmove(
            &mut self,
            sym_dest: Option<RSymExpr>,
            sym_src: Option<RSymExpr>,
            sym_len: Option<RSymExpr>,
            dest: *mut u8,
            src: *const u8,
            length: usize,
        ) {
            if let Some(inner) = &mut self.inner {
                inner.backend_memmove(sym_dest, sym_src, sym_len, dest, src, length);
            }
        }
    };
    (pub fn backend_memset(
        sym_dest: RSymExpr,
        sym_val: RSymExpr,
        sym_len: RSymExpr,
        memory: *mut u8,
        value: ::std::os::raw::c_int,
        length: usize,
    ), $c_name:ident;) => {
        #[allow(clippy::default_trait_access)]
        fn backend_memset(
            &mut self,
            sym_dest: Option<RSymExpr>,
            sym_val: Option<RSymExpr>,
            sym_len: Option<RSymExpr>,
            memory: *mut u8,
            value: ::std::os::raw::c_int,
            length: usize,
        ) {
            if let Some(inner) = &mut self.inner {
                inner.backend_memset(sym_dest, sym_val, sym_len, memory, value, length);
            }
        }
    };

    (pub fn $name:ident($( $arg:ident : $type:ty ),*$(,)?) -> $ret:ty,  $c_name:ident;) => {
        // compile_error!(stringify!($name));
        fn $name(&mut self, $( $arg : $type),*) -> Option<$ret> {
            if let Some(inner) = &mut self.inner {
                inner.$name($($arg,)*)
            } else {
                None
            }
        }
    };

    (pub fn $name:ident($( $arg:ident : $type:ty ),*$(,)?), $c_name:ident;) => {
        // compile_error!(stringify!($name));
        fn $name(&mut self, $( $arg : $type),*) {
            if let Some(inner) = &mut self.inner {
                inner.$name($($arg,)*);
            }
        }
    };
}

impl<RT> Runtime for OptionalRuntime<RT>
where
    RT: Runtime,
{
    invoke_macro_with_rust_runtime_exports!(rust_runtime_function_declaration;);
}

/// This macro allows you to export your runtime from your crate. It is necessary to call this macro in your crate to get a functional runtime.
///
/// ## Simple form
/// The simplest invocation of this macro looks like this:
/// ```no_run
/// # #[macro_use] extern crate symcc_runtime;
/// # use symcc_runtime::{NopRuntime, Runtime};
/// export_runtime!(NopRuntime => NopRuntime);
/// ```
/// The first argument is an expression that constructs your `Runtime` type and the second argument is the name of your `Runtime` type.
/// For example, to construct a tracing runtime, the invocation would look like this:
/// ```no_run
/// # #[macro_use] extern crate symcc_runtime;
/// # use symcc_runtime::{tracing::TracingRuntime, Runtime};
/// export_runtime!(TracingRuntime::new(todo!(), todo!()) => TracingRuntime);
/// ```
///
/// ## Runtime composition using `Filter`s
/// If you're not a fan of macro magic, you should stop reading here.
///
/// To construct a runtime that is composed of [`filter::Filter`]s, you can save some boilerplate code by using the extended form of this macro.
/// The gist of it is that you can prepend any number of `constructor => type` statements (separated by `;`) to your final runtime statement and the result of this macro will wrap your final runtime with the given filters.
/// Filters are applied from left to right.
///
/// Example:
/// ```no_run
/// # #[macro_use] extern crate symcc_runtime;
/// # use symcc_runtime::{tracing::TracingRuntime, Runtime, filter::NoFloat};
/// export_runtime!(NoFloat => NoFloat; TracingRuntime::new(todo!(), todo!()) => TracingRuntime);
/// ```
/// This will construct a runtime that is first filtered by [`filter::NoFloat`] and then traced by the tracing runtime.
///
/// You can achieve the same effect by constructing [`filter::FilterRuntime`] manually, but as you add more filters, the types become tedious to write out.
#[macro_export]
macro_rules! export_runtime {
    // Simple version: just export this runtime
    ($constructor:expr => $rt:ty) => {
        export_runtime!(@final $constructor => $rt);
    };

    // Compositional version: export this chain of filters and a final runtime
    ($filter_constructor:expr => $filter:ty ; $($constructor:expr => $rt:ty);+) => {
        export_runtime!(@final export_runtime!(@combine_constructor $filter_constructor; $($constructor);+) => export_runtime!(@combine_type $filter; $($rt);+));
    };

    // combines a chain of filter constructor expressions
    // recursive case: wrap the constructor expression in a `filter::FilterRuntime::new`
    (@combine_constructor $filter_constructor:expr ; $($constructor:expr);+) => {
        $crate::filter::FilterRuntime::new($filter_constructor, export_runtime!(@combine_constructor $($constructor);+))
    };
    // base case
    (@combine_constructor $constructor:expr) => {
        $constructor
    };

    // combines a chain of filter type expressions
    // recursive case: wrap the type in a `filter::FilterRuntime`
    (@combine_type $filter:ty ; $($rt:ty);+) => {
        $crate::filter::FilterRuntime<$filter, export_runtime!(@combine_type $($rt);+)>
    };
    // base case
    (@combine_type $rt:ty) => {
        $rt
    };

    // finally, generate the necessary code for the given runtime
    (@final $constructor:expr => $rt:ty) => {
        // We are creating a piece of shared mutable state here for our runtime, which is used unsafely.
        // The correct solution here would be to either use a mutex or have per-thread state,
        // however, this is not really supported in SymCC yet.
        // Therefore we make the assumption that there is only ever a single thread, which should
        // mean that this is 'safe'.
        static mut GLOBAL_DATA: Option<$rt> = None;

        #[cfg_attr(not(test), $crate::ctor)]
        fn init() {
            // See comment on GLOBAL_DATA declaration.
            unsafe {
                GLOBAL_DATA = Some($constructor);
                $crate::atexit(fini);
            }
        }

        /// [`libc::atexit`] handler
        extern "C" fn fini() {
            // drops the global data object
            unsafe {
                if let Some(state) = GLOBAL_DATA.take() {
                }
            }
        }

        use $crate::RSymExpr;

        /// A little helper function that encapsulates access to the shared mutable state.
        fn with_state<R>(cb: impl FnOnce(&mut $rt) -> R) -> R {
            use $crate::unchecked_unwrap::UncheckedUnwrap;
            let s = unsafe { GLOBAL_DATA.as_mut().unchecked_unwrap() };
            cb(s)
        }

        $crate::invoke_macro_with_rust_runtime_exports!($crate::export_rust_runtime_fn;with_state);
        #[cfg(target_os="linux")]
        $crate::export_cpp_runtime_functions!();
    };
}
