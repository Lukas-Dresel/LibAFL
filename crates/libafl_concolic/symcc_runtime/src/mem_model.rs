use libafl::observers::concolic::SymExpr;

use crate::RSymExpr;


pub trait ApproximateMemoryModel {
    fn register_new_expr(&mut self, id: RSymExpr, expr: SymExpr);

    fn get_under_approximate_pointer_range(&self, id: RSymExpr) -> Option<(usize, usize)>;

    fn get_exact_pointer_range(&self, id: RSymExpr) -> Option<(usize, usize)>;

    fn get_over_approximate_pointer_range(&self, id: RSymExpr) -> Option<(usize, usize)>;
}