mod call;
mod expr;
mod function;
mod if_statement;
mod loop_statements;
mod parenthesized_expression;
mod statement_list;

pub(crate) use crate::syntax::statement_list::parse_root;
