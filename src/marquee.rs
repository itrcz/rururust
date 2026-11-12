//! One paint-only traversal per hover session, with a quick animated return.
use std::collections::HashMap;

const RETURN_SECONDS:f32=0.18;
#[derive(Default)]
pub(super) struct Marquees { nodes: HashMap<u64, Run> }
struct Run { key:u64, from:f32, to:f32, began:f32, duration:f32, hovered:bool, used:u64 }
impl Run {
    fn sample(&self,now:f32)->(f32,bool) {
        if self.duration<=0.0 {return (self.to,false);}
        let progress=((now-self.began)/self.duration).clamp(0.0,1.0);
        let eased=progress*progress*(3.0-2.0*progress);
        (self.from+(self.to-self.from)*eased,progress<1.0)
    }
}
impl Marquees {
    pub(super) fn offset(&mut self,id:u64,key:u64,distance:f32,speed:f32,hovered:bool,now:f32,frame:u64)->(f32,bool) {
        if self.nodes.get(&id).is_some_and(|run|run.key!=key) {self.nodes.remove(&id);}
        if !self.nodes.contains_key(&id) && (!hovered || distance<=0.5) {return (0.0,false);}
        let run=self.nodes.entry(id).or_insert(Run {
            key,from:0.0,to:distance,began:now,duration:distance/speed,hovered:true,used:frame,
        });
        if run.hovered!=hovered || (hovered && (run.to-distance).abs()>0.5) {
            let (from,_)=run.sample(now);
            let to=if hovered && distance>0.5 {distance}else{0.0};
            let duration=if (to-from).abs()<0.001 {0.0}else if to==0.0 {RETURN_SECONDS}else{(to-from).abs()/speed};
            *run=Run {key,from,to,began:now,duration,hovered,used:frame};
        }
        run.used=frame;
        let result=run.sample(now);
        if !hovered && !result.1 {self.nodes.remove(&id);}
        result
    }

    pub(super) fn end_frame(&mut self,frame:u64) {self.nodes.retain(|_,run|run.used==frame);}
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn one_pass_eases_stops_then_returns_quickly_and_sleeps() {
        let mut m=Marquees::default();
        assert_eq!(m.offset(1,1,100.0,20.0,false,0.0,1),(0.0,false));
        assert_eq!(m.offset(1,1,0.0,20.0,true,0.0,2),(0.0,false));
        assert_eq!(m.offset(1,1,100.0,20.0,true,1.0,3),(0.0,true));
        let (early,running)=m.offset(1,1,100.0,20.0,true,1.5,4);
        assert!(early>0.0 && early<10.0 && running,"starts slower than linear motion");
        assert_eq!(m.offset(1,1,100.0,20.0,true,3.5,5),(50.0,true));
        assert_eq!(m.offset(1,1,100.0,20.0,true,6.0,6),(100.0,false));
        assert_eq!(m.offset(1,1,100.0,20.0,true,30.0,7),(100.0,false));
        assert_eq!(m.offset(1,1,76.0,20.0,false,31.0,8),(100.0,true),"leaving starts from the visible offset despite the wider clip");
        let (middle,running)=m.offset(1,1,76.0,20.0,false,31.09,9);
        assert!((middle-50.0).abs()<0.01 && running);
        assert_eq!(m.offset(1,1,76.0,20.0,false,31.19,10),(0.0,false));assert!(m.nodes.is_empty());
        assert_eq!(m.offset(1,1,100.0,20.0,true,32.0,11),(0.0,true));
        assert_eq!(m.offset(1,2,100.0,20.0,true,33.0,12),(0.0,true),"new text restarts");
        m.end_frame(13);assert!(m.nodes.is_empty());
    }

    #[test]
    fn partial_scroll_and_quick_reentry_reverse_without_jumping() {
        let mut m=Marquees::default();
        m.offset(1,1,100.0,20.0,true,0.0,1);
        let (partial,_)=m.offset(1,1,100.0,20.0,true,2.5,2);
        assert_eq!(m.offset(1,1,0.0,20.0,false,2.5,3),(partial,true),"text fitting at rest still returns smoothly");
        let (returning,_)=m.offset(1,1,0.0,20.0,false,2.59,4);
        let (resumed,running)=m.offset(1,1,100.0,20.0,true,2.59,5);
        assert_eq!(resumed,returning);assert!(resumed>0.0 && resumed<partial && running);
        assert!(m.offset(1,1,100.0,20.0,true,2.7,6).0>resumed);
    }
}
