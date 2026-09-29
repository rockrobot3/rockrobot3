//! rr3-power 骨架：电源（电池 / 充电 / 低压关断兜底）。

mod battery;
mod charger;

fn main() {
    println!("rr3-power 0.0.1 (骨架，未接入总线)");
    for line in battery::SURFACE.iter().chain(charger::SURFACE.iter()) {
        println!("  {line}");
    }
}
