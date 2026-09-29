//! rr3-interact 骨架：交互（表情屏 / TTS 播放 / Web 面板）。

mod face;
mod panel;
mod player;

fn main() {
    println!("rr3-interact 0.0.1 (骨架，未接入总线)");
    for line in face::SURFACE
        .iter()
        .copied()
        .chain(player::SURFACE.iter().copied())
        .chain(panel::surface())
    {
        println!("  {line}");
    }
}
