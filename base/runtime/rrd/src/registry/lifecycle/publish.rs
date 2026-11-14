//! 发布归属判定（三层安全顺序）与拒绝计数、全局低电闸。

use super::super::*;

impl RrdRegistry {
    /// 主题首段（命名空间）：`a/b` → `a`，无分隔符即整串。
    fn rr3_pub_head(topic: &str) -> &str {
        topic.split('/').next().unwrap_or("")
    }

    /// 前三层安全判据的前两层：保留命名空间 → 前缀越权。
    fn rr3_pub_namespace(
        sender_name: &str,
        topic: &str,
        topics: &Rr3TopicSet,
    ) -> Result<(), Rr3PublishDeny> {
        if crate::topics_load::rr3_topics_reserved(topic) {
            return Err(Rr3PublishDeny::ReservedNamespace {
                topic: topic.to_string(),
            });
        }
        if Self::rr3_pub_head(topic) != sender_name {
            return Err(Rr3PublishDeny::ForeignPrefix {
                topic: topic.to_string(),
                owner: owner_for_echo(topics.rr3_topic_owner(topic)),
                sender: sender_name.to_string(),
            });
        }
        Ok(())
    }

    /// 发布归属判定。三层顺序即安全语义：下行冒充 → 前缀越权 → 未在册，`Ok` 之前再把
    /// 主题集里登记的 owner 与发送方比一次（见末段）。
    /// 关联函数（不碰 self），故生产路径与测试路径完全同码。
    ///
    /// 顺序不是洁癖：`rr3-runtime/*` 的冒充者在主题集里可以"合法在册"（owner 就写
    /// `rr3-runtime`），若先查前缀或先查在册，它会得到 ForeignPrefix/Ok 而不是"冒充"这条
    /// 最严重的定性。rrd 自己是该命名空间唯一合法发布者，走的是下行广播路径，不调本函数。
    ///
    /// **`sender_name` 的来源是调用方必须保证的前提，本函数无法替你保证**：签名收
    /// `&str`、不碰 `self`，所以它既不知道这个名字是否在册（从未注册、或已 `Failed`
    /// 的名字照样能拿到 `Ok`），也不知道它是不是报文里编出来的。唯一的防线建在调用侧：
    /// `sender_name` 一律由 `rr3_reg_name_of(src_task)` 从连接持有的 task 号**反查**得到，
    /// **绝不得取自帧体或注册报文里自称的 name 字段**。Task 7 已落地的上行处理（`conn.rs`）与
    /// 此后 Task 8 的发布分支都必须照此接线。
    ///
    /// 末段的 owner 比对不是重复劳动：首段 == `sender_name` 只是主题串的**语法**事实，
    /// `rr3_topic_owner(topic) == sender_name` 才是**在册表登记的事实**。二者今天恒等，
    /// 是因为 `rr3_topic_check` 强制"首段 == owner"，而那条前置只在 `rr3_topics_load`
    /// 的装载路径上被强制；`rr3_topic_parse` 本身是 public，绕开 check
    /// 造一张"首段是 rr3-sima、登记 owner 是 rr3-simb"的表并不费力。安全判定不该押在
    /// "上游一定查过"上，故这里就地把两条都查一遍（多一次 O(log n) 查表，无副作用）。
    pub fn rr3_reg_pub_check(
        sender_name: &str,
        topic: &str,
        topics: &Rr3TopicSet,
    ) -> Result<(), Rr3PublishDeny> {
        Self::rr3_pub_namespace(sender_name, topic, topics)?;
        rr3_pub_registered(topic, topics)?;
        // 首段相符只说明主题串"写着是这个命名空间"；在册 owner 是别人就是越权。
        // 取不到 owner（登记表里 owner 缺失 = 没写这个键，解析成空串）同样按越权拒，
        // 绝不因"查不到"而放行；回显与另一条 ForeignPrefix 分支一致地写 `?`（同走
        // `owner_for_echo`）。
        rr3_pub_owner(sender_name, topic, topics)
    }

    /// 在册 owner 与发送方是否不符。
    pub(crate) fn rr3_owner_mismatch(owner: Option<&str>, sender: &str) -> bool {
        owner != Some(sender)
    }

    /// 记一次越权/被拒发布。只加计数：生命周期状态与别人的转发都不受影响。
    /// 饱和加：计数器撞顶（u32）后停在原值，绝不回绕成 0（回绕等于把审计线索抹平）。
    pub fn rr3_reg_note_denied(&mut self) {
        self.denied_publish = self.denied_publish.saturating_add(1);
    }

    pub fn rr3_reg_denied(&self) -> u32 {
        self.denied_publish
    }

    /// 置全局低电旗。返回是否为**首次**置位（上升沿）——conn 用它保证同一次低电段
    /// 只广播一次 `rr3-runtime/system/power`（重复 battery/low 不重复点名）。
    pub fn rr3_reg_lp_raise(&mut self) -> bool {
        let rising = !self.low_power;
        self.low_power = true;
        rising
    }

    /// 清全局低电旗（批N5 件二笔3，§17 运维留痕"low_power 旗补解除路径"）。
    /// 返回是否为真**下降沿**（此前确实在低电段）——conn 用它保证同一次低电段
    /// 只广播一次 `reason=battery-ok` 的解除点名（重复 battery/ok 不重复广播）。
    /// 与 raise 同一张表同一面：两个沿一个对，谁触发谁点名。
    pub fn rr3_reg_lp_lower(&mut self) -> bool {
        let falling = self.low_power;
        self.low_power = false;
        falling
    }

    /// supervisor 的 spawn 漏斗（`rr3_boot_spawn`）每 spawn 前读这一问。
    pub fn rr3_reg_lp_is_set(&self) -> bool {
        self.low_power
    }
}
