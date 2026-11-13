//! A dismissed form may disappear from the tree immediately. Keep only its final
//! drawing while the entrance spring reverses; no callbacks or model data survive.
use super::*;
use crate::view::ModalStyle;

pub(super) struct ModalFrame {
    cmds: Vec<Cmd>,
    images: Vec<Image>,
    body: usize,
    order: usize,
    underlays: Vec<u64>,
    progress: f32,
    rise: f32,
    style: ModalStyle,
    center: (f32, f32),
    bounds: Rect,
    radius: f32,
    screen: Rect,
    used: u64,
}

impl ModalFrame {
    pub(super) fn retain_text(&self, text: &mut TextSystem) {
        for cmd in &self.cmds {
            if let Cmd::Text(t)=cmd {text.retain_layout(t.key);}
        }
    }
}

impl<S> Painter<'_, S> {
    pub(super) fn modal_fade(&mut self,id:u64,target:f32,style:ModalStyle)->f32 {
        let (k,c)=self.look.theme.motion.fade;
        let duration=if target==0.0 && style.close_slowdown.is_finite() {style.close_slowdown.clamp(0.25,4.0)}else{1.0};
        let spring=self.spring_with(id,slot::FADE,0.0,target,(k,c));
        // Change time scale without resetting position or velocity when a form reverses.
        spring.k=k/(duration*duration);spring.c=c/duration;
        spring.x
    }

    pub(super) fn modal_content_blur(&mut self,bounds:Rect,radius:f32,amount:f32,progress:f32) {
        if !amount.is_finite() || amount<=0.0 || progress<=0.004 {return;}
        let frost=amount.min(12.0)*(1.0-progress);
        if frost<=0.01 {return;}
        // A neutral snapshot pass softens the complete composited panel, including
        // native glyphs and control edges, without rasterizing each label separately.
        let mut xf=Xf::screen(self.screen,Color::CLEAR,0.0);
        let visibility=(progress/0.2).clamp(0.0,1.0);
        xf.opacity=visibility*visibility*(3.0-2.0*visibility);
        self.glass(&xf,bounds.inflate(frost*3.0+2.0),0.0,Glass {
            tint:Color::CLEAR,saturation:1.0,frost,refraction:0.0,depth:0.0,
            dispersion:0.0,rim:0.0,horizontal_light:false,
        },None);
        if let Some(Cmd::Quad(q))=self.list.cmds.last_mut() {
            q.np[3]=FLAG_MODAL_CONTENT as f32;
            q.n0=bounds.arr();q.np[0]=radius;
        }
    }

    pub(super) fn remember_modal(&mut self,id:u64,start:usize,body:usize,progress:f32,style:ModalStyle,parent_scale:f32,area:Rect,bounds:Rect,radius:f32) {
        if progress<=0.004 {return;}
        let underlays=self.modals.iter().filter_map(|(&id,frame)|(frame.used==self.frame).then_some(id)).collect();
        self.modals.insert(id,ModalFrame {cmds:self.list.cmds[start..].to_vec(),
            images:self.list.images.clone(),body:body-start,order:start,underlays,progress,rise:style.rise*parent_scale,style,
            center:(area.x+area.w*0.5,area.y+area.h*0.5),bounds,radius,screen:self.screen,used:self.frame});
    }

    pub(super) fn closing_modals(&mut self) {
        let frames=std::mem::take(self.modals);
        let active:Vec<_>=frames.iter().filter_map(|(&id,frame)|(frame.used==self.frame).then_some(id)).collect();
        let mut frames:Vec<_>=frames.into_iter().collect();
        frames.sort_by_key(|(_,frame)|frame.order);
        for (id,frame) in frames {
            if frame.used==self.frame {self.modals.insert(id,frame);continue;}
            // Keep a dismissed nested panel above its still-open parent, but do not
            // replay a stale panel over a newly opened replacement.
            if active.iter().any(|id|!frame.underlays.contains(id)) {continue;}
            let progress=self.modal_fade(id,0.0,frame.style).clamp(0.0,1.0);
            if progress<=0.004 {continue;}
            let ratio=progress/frame.progress;
            let scale=frame.style.scale_at(progress)/frame.style.scale_at(frame.progress);
            let dy=frame.style.scale_at(progress)*frame.rise*(frame.progress-progress);
            let transform=|r:Rect|Rect::new(frame.center.0+(r.x-frame.center.0)*scale,
                frame.center.1+(r.y-frame.center.1)*scale+dy,r.w*scale,r.h*scale);
            let clip=|r:Rect|if r==frame.screen {r}else{transform(r).intersect(&frame.screen)};
            let images=self.list.images.len();
            self.list.images.extend(frame.images.iter().cloned());
            for (index,cmd) in frame.cmds.iter().enumerate() {
                let body=index>=frame.body;
                let cmd=match cmd {
                    Cmd::Quad(q)=>{
                        let mut q=*q;
                        q.p1[3]*=ratio;
                        if body {
                            q.rect=transform(Rect::new(q.rect[0],q.rect[1],q.rect[2],q.rect[3])).arr();
                            q.clip=clip(Rect::new(q.clip[0],q.clip[1],q.clip[2],q.clip[3])).arr();
                            for value in &mut q.p0[..3] {*value*=scale;}
                            q.p1[2]*=scale;
                            if q.p0[3]==KIND_SHADOW {q.c1[0]*=scale;q.c1[1]*=scale;q.c1[3]*=scale;}
                        }
                        if q.p0[3]==KIND_IMAGE {q.c1[0]+=images as f32;}
                        if q.p0[3]==KIND_GLASS {
                            if !body {
                                q.p1[0]*=ratio;
                                if (q.np[3] as u32)&FLAG_MODAL_BACKDROP!=0 {q.n0[0]*=ratio;}
                            } else {
                                q.p1[0]*=scale;q.c1[1]*=scale;
                                if q.np[2]>0.0 {
                                    q.n0=transform(Rect::new(q.n0[0],q.n0[1],q.n0[2],q.n0[3])).arr();
                                    q.n1=transform(Rect::new(q.n1[0],q.n1[1],q.n1[2],q.n1[3])).arr();
                                    for value in &mut q.np[..3] {*value*=scale;}
                                }
                            }
                        }
                        Cmd::Quad(q)
                    }
                    Cmd::Text(t)=>{
                        let mut t=t.clone();t.color.a*=ratio;
                        if body {
                            let r=transform(Rect::new(t.pos.0,t.pos.1,t.size.0,t.size.1));
                            t.pos=(r.x,r.y);t.size=(r.w,r.h);t.scale*=scale;t.clip=clip(t.clip);
                            t.leading_fade*=scale;
                            if let Some(fade)=&mut t.fade {*fade=transform(*fade);}
                        }
                        Cmd::Text(t)
                    }
                };
                self.list.cmds.push(cmd);
            }
            self.modal_content_blur(transform(frame.bounds),frame.radius*scale,frame.style.content_blur,progress);
            self.hits.push(Hit {id,rect:frame.screen,round:NO_ROUND,action:Action::Swallow});
            self.zones.push(Zone {id,rect:frame.screen,round:NO_ROUND,cursor:None,blocks:true,tip:false});
            self.modals.insert(id,frame);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{modal, rect, zstack, button};

    #[test]
    fn modal_close_slowdown_leaves_opening_unchanged_and_softens_the_tail() {
        const PANEL: Color = Color::rgba(0.31, 0.27, 0.64, 1.0);
        let make = |slowdown| Ui::new(false, move |open: &bool|
            modal(*open, |open: &mut bool| *open = false,
                rect().size(240.0, 100.0).background(PANEL))
                .modal_style(ModalStyle {scale: 1.1, close_slowdown: slowdown, ..Default::default()}));
        let mut normal = make(1.0);
        let mut slow = make(1.6);
        let alpha = |ui: &Ui<bool>| ui.list.cmds.iter().find_map(|cmd| match cmd {
            Cmd::Quad(q) if q.c0 == [PANEL.r, PANEL.g, PANEL.b, PANEL.a] => Some(q.p1[3]),
            _ => None,
        }).unwrap_or(0.0);
        let step = |ui: &mut Ui<bool>| {
            // A first frame uses the fixed 1/120 s fallback. Reset only its timestamp
            // so both UIs integrate identical time, independent of rendering cost and
            // OS scheduling. Springs, targets and retained modal frames remain intact.
            ui.last_frame = None;
            ui.frame((600.0, 400.0));
        };
        step(&mut normal); step(&mut slow);
        normal.state = true; slow.state = true;
        for _ in 0..80 {
            step(&mut normal); step(&mut slow);
            assert_eq!(alpha(&normal), alpha(&slow), "opening timing is unchanged");
        }
        assert_eq!(alpha(&normal), 1.0, "opening must settle before testing its exit");
        normal.state = false; slow.state = false;
        let mut tail = Vec::new();
        // 28 fixed frames cover 233 ms: the same exit interval as 14 frames at 60 Hz.
        for _ in 0..28 {
            step(&mut normal); step(&mut slow);
            tail.push(alpha(&slow));
        }
        assert!(alpha(&slow) > alpha(&normal) + 0.01, "slower closure still has a visible tail");
        assert!(tail[20] - tail[22] > tail[24] - tail[26], "closing decelerates near its end");
        for _ in 0..120 {step(&mut normal); step(&mut slow);}
        assert_eq!(alpha(&slow), 0.0);
        assert!(!slow.animating);
    }
    #[test]
    fn modal_scale_lands_at_center_and_reverses_after_early_dismissal() {
        const PANEL:Color=Color::rgba(0.27,0.43,0.61,1.0);
        let mut ui=Ui::new((false,0usize),|s:&(bool,usize)|zstack((
            button("Background",|s:&mut (bool,usize)|s.1+=10).fill(),
            if s.0 {modal(true,|s:&mut (bool,usize)|s.0=false,
                button("Modal",|s:&mut (bool,usize)|s.1+=1).size(260.0,120.0).background(PANEL))
                .modal_style(ModalStyle {scale:1.1,backdrop_pull:0.12,
                    backdrop:Some(Glass {frost:18.0,..Glass::dark()}),..Default::default()})
            } else {rect().size(0.0,0.0)},
        )));
        let size=(800.0,500.0);
        let step=|ui:&mut Ui<(bool,usize)>| {
            // Advance physics by 1/60 s but paint once, preserving the original target
            // change boundaries. frame() adds the second 1/120 s itself.
            for spring in ui.springs.values_mut() {spring.step(1.0/120.0);}
            ui.last_frame=None;ui.frame(size);
        };
        let panel=|ui:&Ui<(bool,usize)>|ui.list.cmds.iter().find_map(|cmd|match cmd {
            Cmd::Quad(q) if q.c0==[PANEL.r,PANEL.g,PANEL.b,PANEL.a]=>Some(*q),_=>None,
        });
        let backdrop=|ui:&Ui<(bool,usize)>|ui.list.cmds.iter().find_map(|cmd|match cmd {
            Cmd::Quad(q) if (q.np[3] as u32)&131072!=0=>Some(*q),_=>None,
        });
        ui.frame(size);ui.state.0=true;step(&mut ui);step(&mut ui);
        let first=panel(&ui).unwrap();
        assert!(first.rect[2]>260.0*1.07 && first.rect[2]<=286.01, "first rectangle: {:?}", first.rect);
        for _ in 0..3 {step(&mut ui);}
        let early=panel(&ui).unwrap();let blur=backdrop(&ui).unwrap();
        assert!(early.rect[2]<first.rect[2]);
        assert!(blur.n0[0]>0.0 && blur.n0[0]<0.12);
        assert!((early.rect[0]+early.rect[2]/2.0-400.0).abs()<0.01);
        assert!((early.rect[1]+early.rect[3]/2.0-250.0).abs()<0.01);
        ui.pointer_down((400.0,250.0),false);ui.pointer_up((400.0,250.0));assert_eq!(ui.state.1,1);
        ui.state.0=false;
        // The fade spring may finish its inbound velocity before reversing; compare after 6 frames.
        for _ in 0..6 {step(&mut ui);}
        let closing=panel(&ui).unwrap();let restoring=backdrop(&ui).unwrap();
        assert!(closing.rect[2]>early.rect[2]);assert!(closing.p1[3]<early.p1[3]);
        assert!(restoring.n0[0]<blur.n0[0] && restoring.p1[0]<blur.p1[0]);
        assert!((closing.rect[0]+closing.rect[2]/2.0-400.0).abs()<0.01);
        assert!((closing.rect[1]+closing.rect[3]/2.0-250.0).abs()<0.01);
        ui.pointer_down((400.0,250.0),false);ui.pointer_up((400.0,250.0));assert_eq!(ui.state.1,1);
        for _ in 0..60 {step(&mut ui);}
        assert!(panel(&ui).is_none());assert!(!ui.animating);
        ui.state.0=true;for _ in 0..60 {step(&mut ui);}
        let settled=panel(&ui).unwrap();assert_eq!(settled.rect,[270.0,190.0,260.0,120.0]);
        assert!((backdrop(&ui).unwrap().n0[0]-0.12).abs()<0.0001);
        assert!(!ui.animating);
        // Click at the edge, inside the actual transformed control; no stale opening scale.
        ui.pointer_down((275.0,250.0),false);ui.pointer_up((275.0,250.0));assert_eq!(ui.state.1,2);
        ui.key(KeyInput::Escape);assert!(!ui.state.0);
    }

}
