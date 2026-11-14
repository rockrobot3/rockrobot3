//! rr3-brain 协议面：ops 白名单读写与模型行 CRUD/连通性测试。

mod model;
mod ops;

pub(crate) use model::{rr3_model_list, rr3_model_set_arm, rr3_model_test_arm};
pub(crate) use ops::{rr3_model_row_of, rr3_ops_set_arm, rr3_ops_snapshot, rr3_py_str};

#[cfg(test)]
pub(crate) use ops::rr3_ops_write;

#[cfg(test)]
mod tests;
