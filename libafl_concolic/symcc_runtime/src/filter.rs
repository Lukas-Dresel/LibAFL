//! [`Filter`]s are ergonomic abstractions over [`Runtime`] that facilitate filtering expressions.

use std::collections::HashSet;

#[allow(clippy::wildcard_imports)]
use crate::*;

mod coverage;
pub use coverage::{CallStackCoverage, HitmapFilter};

// creates the method declaration and default implementations for the filter trait
macro_rules! rust_filter_function_declaration {
    // expression_unreachable is not supported for filters
    (pub fn expression_unreachable(expressions: *mut RSymExpr, num_elements: usize), $c_name:ident;) => {
    };

    // push_path_constraint is not caught by the following case (because it has not return value),
    // but still needs to return something
    (pub fn push_path_constraint($( $arg:ident : $type:ty ),*$(,)?), $c_name:ident;) => {
        #[allow(unused_variables)]
        fn push_path_constraint(&mut self, $($arg : $type),*) -> bool {
            true
        }
    };

    (pub fn backend_read_memory(
        addr_expr: RSymExpr,
        concolic_read_value: RSymExpr,
        addr: *mut u8,
        length: usize,
        little_endian: bool,
    ) -> RSymExpr, $c_name:ident;) => {
        #[allow(clippy::default_trait_access)]
        #[allow(unused_variables)]
        fn backend_read_memory(
            &mut self,
            addr_expr: Option<RSymExpr>,
            concolic_read_value: Option<RSymExpr>,
            addr: *mut u8,
            length: usize,
            little_endian: bool,
        ) -> bool {
            true
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
        ) -> bool {
            true
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
        #[allow(unused_variables)]
        fn backend_memcpy(
            &mut self,
            sym_dest: Option<RSymExpr>,
            sym_src: Option<RSymExpr>,
            sym_len: Option<RSymExpr>,
            dest: *mut u8,
            src: *const u8,
            length: usize,
        ) -> bool {
            true
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
        #[allow(unused_variables)]
        fn backend_memmove(
            &mut self,
            sym_dest: Option<RSymExpr>,
            sym_src: Option<RSymExpr>,
            sym_len: Option<RSymExpr>,
            dest: *mut u8,
            src: *const u8,
            length: usize,
        ) -> bool {
            true
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
        #[allow(unused_variables)]
        fn backend_memset(
            &mut self,
            sym_dest: Option<RSymExpr>,
            sym_val: Option<RSymExpr>,
            sym_len: Option<RSymExpr>,
            memory: *mut u8,
            value: ::std::os::raw::c_int,
            length: usize,
        ) -> bool {
            true
        }
    };

    (pub fn $name:ident($( $arg:ident : $type:ty ),*$(,)?) -> $ret:ty, $c_name:ident;) => {
        #[allow(unused_variables)]
        fn $name(&mut self, $( $arg : $type),*) -> bool {true}
    };

    (pub fn $name:ident($( $arg:ident : $type:ty ),*$(,)?), $c_name:ident;) => {
        #[allow(unused_variables)]
        fn $name(&mut self, $( $arg : $type),*) {}
    };
}

/// A [`Filter`] can decide for each expression whether the expression should be traced symbolically or be
/// concretized.
///
/// This allows us to implement filtering mechanisms that reduce the amount of traced expressions by
/// concretizing uninteresting expressions.
/// If a filter concretizes an expression that would have later been used as part of another expression that
/// is still symbolic, a concrete instead of a symbolic value is received.
///
/// The interface for a filter matches [`Runtime`] with all methods returning `bool` instead of returning [`Option<RSymExpr>`].
/// Returning `true` indicates that the expression should _continue_ to be processed.
/// Returning `false` indicates that the expression should _not_ be processed any further and its result should be _concretized_.
///
/// For example:
/// Suppose there are symbolic expressions `a` and `b`. Expression `a` is concretized, `b` is still symbolic. If an add
/// operation between `a` and `b` is encountered, it will receive `a`'s concrete value and `b` as a symbolic expression.
///
/// An expression filter also receives code locations (`visit_*` methods) as they are visited in between operations
/// and these code locations are typically used to decide whether an expression should be concretized.
///
/// ## How to use
/// To create your own filter, implement this trait for a new struct.
/// All methods of this trait have default implementations, so you can just implement those methods which you may want
/// to filter.
///
/// Use a [`FilterRuntime`] to compose your filter with a [`Runtime`].
/// ## Example
/// As an example, the following filter concretizes all variables (and, therefore, expressions based on these variables) that are not part of a predetermined set of variables.
/// It is also available to use as [`SelectiveSymbolication`].
/// ```no_run
/// # use symcc_runtime::filter::Filter;
/// # use std::collections::HashSet;
/// struct SelectiveSymbolication {
///     bytes_to_symbolize: HashSet<usize>,
/// }
///
/// impl Filter for SelectiveSymbolication {
///     fn get_input_byte(&mut self, offset: usize, value: u8) -> bool {
///         self.bytes_to_symbolize.contains(&offset)
///     }
///     // Note: No need to implement methods that we are not interested in!
/// }
/// ```
pub trait Filter {
    invoke_macro_with_rust_runtime_exports!(rust_filter_function_declaration;);
}

/// A `FilterRuntime` wraps a [`Runtime`] with a [`Filter`].
///
/// It applies the filter before passing expressions to the inner runtime.
/// It also implements [`Runtime`], allowing for composing multiple [`Filter`]'s in a chain.
#[allow(clippy::module_name_repetitions)]
pub struct FilterRuntime<F, RT> {
    filter: F,
    runtime: RT,
}

impl<F, RT> FilterRuntime<F, RT> {
    pub fn new(filter: F, runtime: RT) -> Self {
        Self { filter, runtime }
    }
}

macro_rules! rust_filter_function_implementation {
    (pub fn expression_unreachable(expressions: *mut RSymExpr, num_elements: usize), $c_name:ident;) => {
        fn expression_unreachable(&mut self, exprs: &[RSymExpr]) {
            self.runtime.expression_unreachable(exprs)
        }
    };

    (pub fn push_path_constraint($( $arg:ident : $type:ty ),*$(,)?), $c_name:ident;) => {
        fn push_path_constraint(&mut self, $($arg : $type),*) {
            if self.filter.push_path_constraint($($arg),*) {
                self.runtime.push_path_constraint($($arg),*)
            }
        }
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
        ) -> Option<RSymExpr> {
            if self.filter.backend_read_memory(
                addr_expr,
                concolic_read_value,
                addr,
                length,
                little_endian,
            ) {
                self.runtime.backend_read_memory(addr_expr, concolic_read_value, addr, length, little_endian)
            } else {
                concolic_read_value
            }
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
            if self.filter.backend_write_memory(
                symbolic_addr_expr,
                written_expr,
                concrete_addr,
                concrete_length,
                little_endian,
            ) {
                self.runtime.backend_write_memory(
                    symbolic_addr_expr,
                    written_expr,
                    concrete_addr,
                    concrete_length,
                    little_endian,
                )
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
            if self.filter.backend_memcpy(sym_dest, sym_src, sym_len, dest, src, length) {
                self.runtime.backend_memcpy(sym_dest, sym_src, sym_len, dest, src, length)
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
            if self.filter.backend_memmove(sym_dest, sym_src, sym_len, dest, src, length) {
                self.runtime.backend_memmove(sym_dest, sym_src, sym_len, dest, src, length)
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
            if self.filter.backend_memset(sym_dest, sym_val, sym_len, memory, value, length) {
                self.runtime.backend_memset(sym_dest, sym_val, sym_len, memory, value, length)
            }
        }
    };

    (pub fn $name:ident($( $arg:ident : $type:ty ),*$(,)?) -> $ret:ty, $c_name:ident;) => {
        fn $name(&mut self, $($arg : $type),*) -> Option<$ret> {
            if self.filter.$name($($arg),*) {
                self.runtime.$name($($arg),*)
            } else {
                None
            }
        }
    };

    (pub fn $name:ident($( $arg:ident : $type:ty ),*$(,)?), $c_name:ident;) => {
        fn $name(&mut self, $( $arg : $type),*) {
            self.filter.$name($($arg),*);
            self.runtime.$name($($arg),*);
        }
    };
}

impl<F, RT> Runtime for FilterRuntime<F, RT>
where
    F: Filter,
    RT: Runtime,
{
    invoke_macro_with_rust_runtime_exports!(rust_filter_function_implementation;);
}

/// A [`Filter`] that concretizes all input byte expressions that are not included in a predetermined set of
/// of input byte offsets.
pub struct SelectiveSymbolication {
    bytes_to_symbolize: HashSet<usize>,
}

impl SelectiveSymbolication {
    #[must_use]
    pub fn new(offset: HashSet<usize>) -> Self {
        Self {
            bytes_to_symbolize: offset,
        }
    }
}

impl Filter for SelectiveSymbolication {
    fn get_input_byte(&mut self, offset: usize, _value: u8) -> bool {
        self.bytes_to_symbolize.contains(&offset)
    }
}

/// Concretizes all floating point operations.
pub struct NoFloat;

impl Filter for NoFloat {
    fn build_float(&mut self, _value: f64, _is_double: bool) -> bool {
        false
    }
    fn build_float_ordered(&mut self, _a: RSymExpr, _b: RSymExpr) -> bool {
        false
    }
    fn build_float_ordered_equal(&mut self, _a: RSymExpr, _b: RSymExpr) -> bool {
        false
    }
    fn build_float_ordered_greater_equal(&mut self, _a: RSymExpr, _b: RSymExpr) -> bool {
        false
    }
    fn build_float_ordered_greater_than(&mut self, _a: RSymExpr, _b: RSymExpr) -> bool {
        false
    }
    fn build_float_ordered_less_equal(&mut self, _a: RSymExpr, _b: RSymExpr) -> bool {
        false
    }
    fn build_float_ordered_less_than(&mut self, _a: RSymExpr, _b: RSymExpr) -> bool {
        false
    }
    fn build_float_ordered_not_equal(&mut self, _a: RSymExpr, _b: RSymExpr) -> bool {
        false
    }
    fn build_float_to_bits(&mut self, _expr: RSymExpr) -> bool {
        false
    }
    fn build_float_to_float(&mut self, _expr: RSymExpr, _to_double: bool) -> bool {
        false
    }
    fn build_float_to_signed_integer(&mut self, _expr: RSymExpr, _bits: u8) -> bool {
        false
    }
    fn build_float_to_unsigned_integer(&mut self, _expr: RSymExpr, _bits: u8) -> bool {
        false
    }
    fn build_float_unordered(&mut self, _a: RSymExpr, _b: RSymExpr) -> bool {
        false
    }
    fn build_float_unordered_equal(&mut self, _a: RSymExpr, _b: RSymExpr) -> bool {
        false
    }
    fn build_float_unordered_greater_equal(&mut self, _a: RSymExpr, _b: RSymExpr) -> bool {
        false
    }
    fn build_float_unordered_greater_than(&mut self, _a: RSymExpr, _b: RSymExpr) -> bool {
        false
    }
    fn build_float_unordered_less_equal(&mut self, _a: RSymExpr, _b: RSymExpr) -> bool {
        false
    }
    fn build_float_unordered_less_than(&mut self, _a: RSymExpr, _b: RSymExpr) -> bool {
        false
    }
    fn build_float_unordered_not_equal(&mut self, _a: RSymExpr, _b: RSymExpr) -> bool {
        false
    }
    fn build_int_to_float(&mut self, _value: RSymExpr, _is_double: bool, _is_signed: bool) -> bool {
        false
    }
    fn build_bits_to_float(&mut self, _expr: RSymExpr, _to_double: bool) -> bool {
        false
    }
    fn build_fp_abs(&mut self, _a: RSymExpr) -> bool {
        false
    }
    fn build_fp_add(&mut self, _a: RSymExpr, _b: RSymExpr) -> bool {
        false
    }
    fn build_fp_sub(&mut self, _a: RSymExpr, _b: RSymExpr) -> bool {
        false
    }
    fn build_fp_mul(&mut self, _a: RSymExpr, _b: RSymExpr) -> bool {
        false
    }
    fn build_fp_div(&mut self, _a: RSymExpr, _b: RSymExpr) -> bool {
        false
    }
    fn build_fp_rem(&mut self, _a: RSymExpr, _b: RSymExpr) -> bool {
        false
    }
    fn build_fp_neg(&mut self, _a: RSymExpr) -> bool {
        false
    }
}

pub struct NoMem;
impl Filter for NoMem {
    fn backend_memcpy(&mut self,sym_dest:Option<RSymExpr>,sym_src:Option<RSymExpr>,sym_len:Option<RSymExpr>,dest: *mut u8,src: *const u8,length:usize,) -> bool {
        false
    }
    fn backend_memset(&mut self,sym_dest:Option<RSymExpr>,sym_val:Option<RSymExpr>,sym_len:Option<RSymExpr>,dest: *mut u8,val: i32, length:usize,) -> bool {
        false
    }
    fn backend_memmove(&mut self,sym_dest:Option<RSymExpr>,sym_src:Option<RSymExpr>,sym_len:Option<RSymExpr>,dest: *mut u8,src: *const u8,length:usize,) -> bool {
        false
    }
    fn backend_read_memory(&mut self,addr_expr:Option<RSymExpr>,concolic_read_value:Option<RSymExpr>,addr: *mut u8,length:usize,little_endian:bool,) -> bool {
        false
    }
    fn backend_write_memory(&mut self,addr_expr:Option<RSymExpr>,concolic_write_value:Option<RSymExpr>,addr: *mut u8,length:usize,little_endian:bool,) -> bool {
        false
    }
}
