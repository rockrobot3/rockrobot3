// registry 的生命周期面：注册/索引维护、心跳提升、到期判活、发布归属判定、
// 低电闸、死亡与重启记账、降级/恢复、子进程收尸。全部是 `RrdRegistry` 的
// 有状态方法——纯策略在 model.rs，只读视图在 view.rs。

mod charge;
mod child_exit;
mod counters;
mod death;
mod degrade;
mod expire;
mod publish;
mod register;
