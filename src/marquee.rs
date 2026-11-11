//! One paint-only traversal of an overflowing label for each hover session.
use std::collections::HashMap;

#[derive(Default)]
pub(super) struct Marquees { nodes: HashMap<u64, Run> }
struct Run { key: u64, distance: f32, began: f32, used: u64 }

impl Marquees {
    pub(super) fn offset(&mut self,id:u64,key:u64,distance:f32,speed:f32,hovered:bool,now:f32,frame:u64)->(f32,bool) {
        if !hovered || distance<=0.5 {
            self.nodes.remove(&id);
            return (0.0,false);
        }
        let run=self.nodes.entry(id).or_insert(Run {key,distance,began:now,used:frame});
        if run.key!=key || (run.distance-distance).abs()>0.5 {
            *run=Run {key,distance,began:now,used:frame};
        }
        run.used=frame;
        let progress=((now-run.began)/(distance/speed)).clamp(0.0,1.0);
        let eased=progress*progress*(3.0-2.0*progress);
        (distance*eased,progress<1.0)
    }

    pub(super) fn end_frame(&mut self,frame:u64) {self.nodes.retain(|_,run|run.used==frame);}
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn one_pass_eases_stops_resets_and_does_not_run_for_fitting_text() {
        let mut m=Marquees::default();
        assert_eq!(m.offset(1,1,100.0,20.0,false,0.0,1),(0.0,false));
        assert_eq!(m.offset(1,1,0.0,20.0,true,0.0,2),(0.0,false));
        assert_eq!(m.offset(1,1,100.0,20.0,true,1.0,3),(0.0,true));
        let (early,running)=m.offset(1,1,100.0,20.0,true,1.5,4);
        assert!(early>0.0 && early<10.0 && running,"starts slower than linear motion");
        assert_eq!(m.offset(1,1,100.0,20.0,true,3.5,5),(50.0,true));
        assert_eq!(m.offset(1,1,100.0,20.0,true,6.0,6),(100.0,false));
        assert_eq!(m.offset(1,1,100.0,20.0,true,30.0,7),(100.0,false));
        assert_eq!(m.offset(1,1,100.0,20.0,false,31.0,8),(0.0,false));
        assert_eq!(m.offset(1,1,100.0,20.0,true,32.0,9),(0.0,true));
        assert_eq!(m.offset(1,2,100.0,20.0,true,33.0,10),(0.0,true),"new text restarts");
        m.end_frame(11);assert!(m.nodes.is_empty());
    }
}
