//! Pointer-following glass demo; click the button to simulate file hover on the web.
use ruru::prelude::*;
fn main() {
    App::new(false,|active:&bool,_:&Env|vstack((
        button("Toggle file hover",|active:&mut bool|*active=!*active),
        zstack((
            vstack((0..12).map(|n|text(format!("Conversation row {n} — drag a file or move the pointer"))
                .font_size(20.0).fill_width()).collect::<Vec<_>>()).spacing(18.0).padding(24.0).fill(),
            rect().fill().glass_with(Glass{tint:Color::CLEAR,frost:0.8,
                refraction:0.0,rim:0.0,depth:0.0,dispersion:0.0,saturation:1.0,horizontal_light:false})
                .glass_attraction(0.45,8.0).fade(*active),
        )).fill().clip(),
    )).fill()).size(900.0,720.0).on_file_hover(|s,active|*s=active).run();
}
