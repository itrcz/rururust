//! Timing for the tail activity marker, independent of painting and GPU state.
#[derive(Clone,Copy,Debug)]
pub(super) struct Pose {pub shape:f32,pub rotation:f32,pub animate:bool,pub wake:Option<f32>}
pub(super) fn pose(time:f32,period:f32,duration:f32)->Pose {
    if period<=0.0 || duration<=0.0 {return Pose {shape:0.0,rotation:0.0,animate:false,wake:None};}
    let duration=duration.min(period);let phase=time.rem_euclid(period);
    let shape=(time/period).floor().rem_euclid(3.0);
    let hold=period-duration;
    if phase<hold {return Pose {shape,rotation:0.0,animate:false,wake:Some(hold-phase)};}
    let progress=(phase-hold)/duration;
    let ease=progress*progress*(3.0-2.0*progress);
    Pose {shape:shape+ease,rotation:0.12*(ease*std::f32::consts::PI).sin(),animate:true,wake:None}
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn morph_cycles_all_shapes_with_smooth_endpoints_and_sleeping_holds() {
        for index in 0..3 {
            let at=index as f32*2.2;
            let hold=pose(at+0.5,2.2,0.55);
            assert!((hold.shape-index as f32).abs()<0.001);assert!(!hold.animate);assert!(hold.wake.unwrap()>1.0);assert_eq!(hold.rotation,0.0);
            let middle=pose(at+1.925,2.2,0.55);
            assert!((middle.shape-(index as f32+0.5)).abs()<0.001);assert!(middle.animate);assert!(middle.wake.is_none());assert!((middle.rotation-0.12).abs()<0.001);
            let start=pose(at+1.65001,2.2,0.55);let end=pose(at+2.19999,2.2,0.55);
            assert!((start.shape-index as f32).abs()<0.001);assert!((end.shape-index as f32-1.0).abs()<0.001);
            assert!(start.rotation.abs()<0.001&&end.rotation.abs()<0.001);
        }
        assert!((pose(6.7,2.2,0.55).shape).abs()<0.001);
    }
}
