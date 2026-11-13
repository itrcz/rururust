//! A dismissed form may disappear from the tree immediately. Keep only its final
//! drawing while the entrance spring reverses; no callbacks or model data survive.
use super::*;

pub(super) struct ModalFrame {
    cmds: Vec<Cmd>,
    images: Vec<Image>,
    body: usize,
    progress: f32,
    rise: f32,
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
    pub(super) fn remember_modal(&mut self,id:u64,start:usize,body:usize,progress:f32,rise:f32) {
        if progress<=0.004 {return;}
        self.modals.insert(id,ModalFrame {cmds:self.list.cmds[start..].to_vec(),
            images:self.list.images.clone(),body:body-start,progress,rise,screen:self.screen,used:self.frame});
    }

    pub(super) fn closing_modals(&mut self) {
        let frames=std::mem::take(self.modals);
        let another_open=frames.values().any(|frame|frame.used==self.frame);
        for (id,frame) in frames {
            if frame.used==self.frame {self.modals.insert(id,frame);continue;}
            if another_open {continue;}
            let progress=self.fade(id,0.0).clamp(0.0,1.0);
            if progress<=0.004 {continue;}
            let ratio=progress/frame.progress;
            let dy=frame.rise*(frame.progress-progress);
            let images=self.list.images.len();
            self.list.images.extend(frame.images.iter().cloned());
            for (index,cmd) in frame.cmds.iter().enumerate() {
                let body=index>=frame.body;
                let shift=if body {dy}else{0.0};
                let cmd=match cmd {
                    Cmd::Quad(q)=>{
                        let mut q=*q;
                        q.rect[1]+=shift;q.clip[1]+=shift;q.p1[3]*=ratio;
                        if q.p0[3]==KIND_IMAGE {q.c1[0]+=images as f32;}
                        if q.p0[3]==KIND_GLASS {
                            if !body {q.p1[0]*=ratio;}
                            if q.np[2]>0.0 {q.n0[1]+=shift;q.n1[1]+=shift;}
                        }
                        Cmd::Quad(q)
                    }
                    Cmd::Text(t)=>{
                        let mut t=t.clone();t.pos.1+=shift;t.clip.y+=shift;t.color.a*=ratio;
                        if let Some(fade)=&mut t.fade {fade.y+=shift;}
                        Cmd::Text(t)
                    }
                };
                self.list.cmds.push(cmd);
            }
            self.hits.push(Hit {id,rect:frame.screen,round:NO_ROUND,action:Action::Swallow});
            self.zones.push(Zone {id,rect:frame.screen,round:NO_ROUND,cursor:None,blocks:true,tip:false});
            self.modals.insert(id,frame);
        }
    }
}
