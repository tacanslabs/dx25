// FIXME: concordium's address is Copy, but NEAR's one is not
#![allow(clippy::clone_on_copy)]
// Won't be fixed - `|x| x.do_something()` is usually more readable
#![allow(clippy::redundant_closure_for_method_calls)]

mod base;
mod deposit_execute_actions;
mod execute_actions;
mod execute_actions_impl;
mod execute_swap_action;

use super::super::super::dex;
