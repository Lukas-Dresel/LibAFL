use itertools::Itertools;
use libafl_bolts::prelude::{fork, ForkResult};
use libafl::{observers::concolic::{SymExprRef, SymExpr, Location}};
use z3::ast::{Dynamic, BV, Bool, Ast};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ConstraintMutationSource {
    ConcretizePointer { expr: SymExprRef, value: usize, location: Location },
    ConcretizeSize { expr: SymExprRef, value: usize, location: Location },
    PathConstraint { expr: SymExprRef, taken: bool, location: Location },
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ConstraintToMutate<'z3_ctx> {

    pub is_divergent: bool,

    // the source of the constraint + how many other constraints originated from this exact place as previously (e.g. for concretization it's 0 for the < constraint, 1 for the >, and 2 for the == constraint)
    pub source: (ConstraintMutationSource, usize),

    pub constraint: Bool<'z3_ctx>,
    pub constraint_guard: Bool<'z3_ctx>,
    pub location: Location,
    pub key: ConstraintKey,
}
pub struct InterpretedTrace<'z3_ctx> {
    ctx: &'z3_ctx z3::Context,
    msgs: Vec<(SymExprRef, SymExpr)>,
    translation: Vec<Dynamic<'z3_ctx>>,
    mutation_sources: Vec<ConstraintMutationSource>,
    constraints_for_mutation: Vec<ConstraintToMutate<'z3_ctx>>,
}

pub type ConstraintId = usize;
pub type ConstraintKey = (ConstraintId, usize);

impl<'z3_ctx> InterpretedTrace<'z3_ctx> {
    pub fn as_bool(&self, op: SymExprRef) -> Bool<'z3_ctx> {
        if let Some(bool_val) = self.translation[op.get() - 1].as_bool() {
            bool_val
        } else {
            let bv = self.translation[op.get() - 1].as_bv().unwrap();
            let sz = bv.get_size();
            assert!(sz == 1, "_self.as_bool() called on non-bool expr: {:?} of size {}", bv, sz);
            bv._eq(&BV::from_i64(bv.get_ctx(), 0, sz)).not()
        }
    }

    pub fn as_bv(&self, op: SymExprRef) -> BV<'z3_ctx> {
        match self.translation[op.get() - 1].as_bv() {
            Some(bv) => bv,
            None => {
                let bool_val = self.translation[op.get() - 1].as_bool().unwrap();
                bool_val.ite(&BV::from_i64(bool_val.get_ctx(), 1, 1), &BV::from_i64(bool_val.get_ctx(), 0, 1))
            }
        }
    }

    pub fn unique_constraints_iter(&self) -> impl Iterator<Item = &ConstraintToMutate<'z3_ctx>> {
        self.constraints_for_mutation
            .iter()
            .unique_by(|x| x.key)
    }



    pub fn from_messages_crash_resistant(ctx: &'z3_ctx z3::Context, msgs: Vec<(SymExprRef, SymExpr)>, get_symbolic_byte_var: &mut dyn FnMut(usize) -> BV<'z3_ctx>) -> Result<Self, String> {
        // fork a canary child process that tells us whether or not this will succeed
        match unsafe { fork() } {
            Ok(ForkResult::Parent(child_handle)) => {
                if child_handle.status() == 0 {
                    Ok(InterpretedTrace::from_messages(ctx, msgs, get_symbolic_byte_var))
                } else {
                    Err(format!("forked child failed with status {}", child_handle.status()))
                }
            },
            Ok(ForkResult::Child) => {
                let _self = InterpretedTrace::from_messages(ctx, msgs, get_symbolic_byte_var);
                std::process::exit(0);
            },
            Err(e) => {
                return Err(format!("fork failed: {}", e));
            }
        }
    }





    fn from_messages(ctx: &'z3_ctx z3::Context, msgs: Vec<(SymExprRef, SymExpr)>, get_symbolic_byte_var: &mut dyn FnMut(usize) -> BV<'z3_ctx>) -> Self {
        let mut _self = InterpretedTrace {
            ctx,
            msgs,
            translation: vec![],
            mutation_sources: vec![],
            constraints_for_mutation: vec![],
        };

        macro_rules! bv_binop {
            ($a:ident $op:tt $b:ident) => {
                Some(_self.as_bv($a).$op(&_self.as_bv($b)).into())
            };
        }

        // let last_msgid = _self.msgs.last().unwrap().0.get();

        for (id, msg) in &_self.msgs {
            let id = *id;
            let msg = msg.clone();
            let z3_expr: Option<Dynamic> = match msg {
                SymExpr::PathConstraint { constraint, taken, location } => {
                    let mutation_source = ConstraintMutationSource::PathConstraint {
                        expr: constraint, taken: taken, location: location
                    };
                    let cst_expr = _self.as_bool(constraint).simplify();
                    let cst_expr_not = cst_expr.not().simplify();
                    let (expr_taken, expr_divergent) = if taken {
                        (cst_expr, cst_expr_not)
                    }
                    else {
                        (cst_expr_not, cst_expr)
                    };

                    let cst_guard_taken = Bool::new_const(&ctx, format!("path_constraint_0x{:x}_taken_{:x?}", constraint.get(), location));
                    let cst_guard_divergent = Bool::new_const(&ctx, format!("path_constraint_0x{:x}_divergent_{:x?}", constraint.get(), location));

                    _self.mutation_sources.push(mutation_source.clone());
                    _self.constraints_for_mutation.push(
                        ConstraintToMutate {
                            is_divergent: true,
                            source: (mutation_source.clone(), 0),
                            constraint: expr_divergent,
                            constraint_guard: cst_guard_divergent,
                            location: location.clone(),
                            key: (constraint.get(), 0),
                        });
                    _self.constraints_for_mutation.push(
                        ConstraintToMutate {
                            is_divergent: false,
                            source: (mutation_source.clone(), 1),
                            constraint: expr_taken,
                            constraint_guard: cst_guard_taken,
                            location: location.clone(),
                            key: (constraint.get(), 1),
                        });
                    None
                },
                SymExpr::ConcretizePointer { expr, value, location } => {
                    #[cfg(not(feature="solver_enforce_concretization"))]
                    {
                        let _expr = expr;
                        let _value = value;
                        let _location = location;
                    }
                    #[cfg(feature="solver_enforce_concretization")]
                    {
                        let mutation_source = match msg {
                            SymExpr::ConcretizePointer { expr, value, location } => {
                                ConstraintMutationSource::ConcretizePointer {
                                    expr: expr, value: value, location: location
                                }
                            },
                            SymExpr::ConcretizePointer { expr, value, location } => {
                                ConstraintMutationSource::ConcretizePointer {
                                    expr: expr, value: value, location: location
                                }
                            },
                            _ => unreachable!()
                        };

                        let val_expr = _self.as_bv(expr).simplify();
                        let _lt_cst = val_expr.bvult(&BV::from_u64(&ctx, value as u64, usize::BITS.try_into().unwrap())).simplify();
                        let _gt_cst = val_expr.bvugt( &BV::from_u64(&ctx, value as u64, usize::BITS.try_into().unwrap())).simplify();
                        let _eq_cst = val_expr._eq(&BV::from_u64(&ctx, value as u64, usize::BITS.try_into().unwrap())).simplify();
                        let _lt_guard = Bool::new_const(&ctx, format!("concretize_pointer_0x{:x}_divergent_lt_{:x?}", expr.get(), location));
                        let _gt_guard = Bool::new_const(&ctx, format!("concretize_pointer_0x{:x}_divergent_gt_{:x?}", expr.get(), location));
                        let _eq_guard = Bool::new_const(&ctx, format!("concretize_pointer_0x{:x}_eq_{:x?}", expr.get(), location));

                        _self.constraints_for_mutation.push(
                            ConstraintToMutate {
                                is_divergent: true,
                                source: (mutation_source.clone(), 0),
                                constraint: lt_cst,
                                constraint_guard: lt_guard,
                                location: location.clone(),
                                key: (expr.get(), 0),
                            });

                        _self.constraints_for_mutation.push(
                            ConstraintToMutate {
                                is_divergent: true,
                                source: (mutation_source.clone(), 1),
                                constraint: gt_cst,
                                constraint_guard: gt_guard,
                                location: location.clone(),
                                key: (expr.get(), 1),
                            });

                        _self.path_constraints.push(
                            ConstraintToMutate {
                                is_divergent: false,
                                source: (mutation_source.clone(), 2),
                                constraint: eq_cst,
                                constraint_guard: eq_guard,
                                location: location.clone(),
                                key: (expr.get(), 2),
                            });
                    }
                    None
                },

                SymExpr::InputByte { offset, .. } => {
                    // assert!(next_var_index <= offset);
                    // next_var_index = offset + 1;
                    Some(get_symbolic_byte_var(offset).into())
                }
                SymExpr::Integer { value, bits } => {
                    Some(BV::from_u64(&ctx, value, bits.try_into().unwrap()).into())
                }
                SymExpr::Integer128 { high: _, low: _ } => todo!(),
                SymExpr::NullPointer => {
                    Some(BV::from_u64(&ctx, 0, usize::BITS.try_into().unwrap()).into())
                }
                SymExpr::True => Some(Bool::from_bool(&ctx, true).into()),
                SymExpr::False => Some(Bool::from_bool(&ctx, false).into()),
                SymExpr::Bool { value } => Some(Bool::from_bool(&ctx, value).into()),
                SymExpr::Neg { op } => Some(_self.as_bv(op).bvneg().into()),
                SymExpr::Add { a, b } => bv_binop!(a bvadd b),
                SymExpr::Sub { a, b } => bv_binop!(a bvsub b),
                SymExpr::Mul { a, b } => bv_binop!(a bvmul b),
                SymExpr::UnsignedDiv { a, b } => bv_binop!(a bvudiv b),
                SymExpr::SignedDiv { a, b } => bv_binop!(a bvsdiv b),
                SymExpr::UnsignedRem { a, b } => bv_binop!(a bvurem b),
                SymExpr::SignedRem { a, b } => bv_binop!(a bvsrem b),
                SymExpr::ShiftLeft { a, b } => bv_binop!(a bvshl b),
                SymExpr::LogicalShiftRight { a, b } => bv_binop!(a bvlshr b),
                SymExpr::ArithmeticShiftRight { a, b } => bv_binop!(a bvashr b),
                SymExpr::SignedLessThan { a, b } => bv_binop!(a bvslt b),
                SymExpr::SignedLessEqual { a, b } => bv_binop!(a bvsle b),
                SymExpr::SignedGreaterThan { a, b } => bv_binop!(a bvsgt b),
                SymExpr::SignedGreaterEqual { a, b } => bv_binop!(a bvsge b),
                SymExpr::UnsignedLessThan { a, b } => bv_binop!(a bvult b),
                SymExpr::UnsignedLessEqual { a, b } => bv_binop!(a bvule b),
                SymExpr::UnsignedGreaterThan { a, b } => bv_binop!(a bvugt b),
                SymExpr::UnsignedGreaterEqual { a, b } => bv_binop!(a bvuge b),
                SymExpr::Not { op } => {
                    let translated = &_self.translation[op.get() - 1];
                    Some(if let Some(bv) = translated.as_bv() {
                        bv.bvnot().into()
                    } else if let Some(bool) = translated.as_bool() {
                        bool.not().into()
                    } else {
                        panic!(
                            "unexpected z3 expr of type {:?} when applying not operation",
                            translated.kind()
                        )
                    })
                }
                SymExpr::Equal { a, b } => Some(_self.translation[a.get() - 1]._eq(&_self.translation[b.get() - 1]).into()),
                SymExpr::NotEqual { a, b } => Some(_self.translation[a.get() - 1]._eq(&_self.translation[b.get() - 1]).not().into()),
                SymExpr::BoolAnd { a, b } => Some(Bool::and(&ctx, &[&_self.as_bool(a), &_self.as_bool(b)]).into()),
                SymExpr::BoolOr { a, b } => Some(Bool::or(&ctx, &[&_self.as_bool(a), &_self.as_bool(b)]).into()),
                SymExpr::BoolXor { a, b } => Some(_self.as_bool(a).xor(&_self.as_bool(b)).into()),
                SymExpr::And { a, b } => bv_binop!(a bvand b),
                SymExpr::Or { a, b } => bv_binop!(a bvor b),
                SymExpr::Xor { a, b } => bv_binop!(a bvxor b),
                SymExpr::Sext { op, bits } => Some(_self.as_bv(op).sign_ext(u32::from(bits)).into()),
                SymExpr::Zext { op, bits } => Some(_self.as_bv(op).zero_ext(u32::from(bits)).into()),
                SymExpr::Trunc { op, bits } => Some(_self.as_bv(op).extract(u32::from(bits - 1), 0).into()),
                SymExpr::BoolToBit { op } => Some(
                    _self.as_bool(op)
                        .ite(
                            &BV::from_u64(&ctx, 1, 1),
                            &BV::from_u64(&ctx, 0, 1),
                        )
                        .into(),
                ),
                SymExpr::Concat { a, b } => bv_binop!(a concat b),
                SymExpr::Extract {
                    op,
                    first_bit,
                    last_bit,
                } => Some(_self.as_bv(op).extract(first_bit as u32, last_bit as u32).into()),
                SymExpr::Insert {
                    target,
                    to_insert,
                    offset,
                    little_endian,
                } => {
                    let target = _self.as_bv(target);
                    let to_insert = _self.as_bv(to_insert);
                    let bits_to_insert: u64 = to_insert.get_size().try_into().unwrap();
                    assert_eq!(bits_to_insert % 8, 0, "can only insert full bytes");
                    let target_size: u64 = target.get_size().try_into().unwrap();
                    let after_len = (target_size / 8) - offset - (bits_to_insert / 8);
                    Some(
                        [
                            if offset == 0 {
                                None
                            } else {
                                Some(build_extract(&target, 0, offset, false))
                            },
                            Some(if little_endian {
                                build_extract(&to_insert, 0, bits_to_insert / 8, true)
                            } else {
                                to_insert
                            }),
                            if after_len == 0 {
                                None
                            } else {
                                Some(build_extract(
                                    &target,
                                    offset + (bits_to_insert / 8),
                                    after_len,
                                    false,
                                ))
                            },
                        ]
                        .into_iter()
                        .reduce(|acc: Option<BV>, val: Option<BV>| match (acc, val) {
                            (Some(prev), Some(next)) => Some(prev.concat(&next)),
                            (Some(prev), None) => Some(prev),
                            (None, next) => next,
                        })
                        .unwrap()
                        .unwrap()
                        .into(),
                    )
                },
                SymExpr::SymbolicMemoryRead { value_read_expr, .. } => if let Some(value_read) = &value_read_expr {
                    Some(_self.translation[value_read.get() - 1].clone())
                } else {
                    None
                },
                _ => None,
            };
            if let Some(expr) = z3_expr {
                // println!("Inserting expression {:x} => {}", id, expr);
                assert!(_self.translation.len() == id.get() - 1);
                _self.translation.push(expr);
            }
        }

        _self
    }
}

fn build_extract<'ctx>(
    bv: &BV<'ctx>,
    offset: u64,
    length: u64,
    little_endian: bool,
) -> BV<'ctx> {
    let size: u64 = bv.get_size().try_into().unwrap();
    assert_eq!(
        size % 8,
        0,
        "can't extract on byte-boundary on BV that is not byte-sized"
    );

    if little_endian {
        (0..length)
            .map(|i| {
                bv.extract(
                    (size - (offset + i) * 8 - 1).try_into().unwrap(),
                    (size - (offset + i + 1) * 8).try_into().unwrap(),
                )
            })
            .reduce(|acc, next| next.concat(&acc))
            .unwrap()
    } else {
        bv.extract(
            (size - offset * 8 - 1).try_into().unwrap(),
            (size - (offset + length) * 8).try_into().unwrap(),
        )
    }
}