//! rr3-act 骨架：执行（舵机 / LED / 反射环宿主）。

mod led;
mod reflex;
mod servo;

fn main() {
    println!("rr3-act 0.0.1 (骨架，未接入总线)");
    for line in servo::SURFACE
        .iter()
        .chain(led::SURFACE.iter())
        .chain(reflex::SURFACE.iter())
    {
        println!("  {line}");
    }
}
