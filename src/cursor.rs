//! Timing for the circle/triangle tail marker, independent of painting and GPU state.
#[derive(Clone,Copy,Debug)]
pub(super) struct Pose {pub shape:f32,pub rotation:f32,pub scale:f32,pub animate:bool,pub wake:Option<f32>}
pub(super) fn pose(time:f32,period:f32,duration:f32)->Pose {
    if period<=0.0 || duration<=0.0 {return Pose {shape:2.0,rotation:0.0,scale:1.0,animate:false,wake:None};}
    let duration=duration.min(period*0.25);
    let triangle_hold=period*0.2;
    let circle_hold=period-2.0*duration-triangle_hold;
    let phase=time.rem_euclid(period);
    if phase<circle_hold {
        let scale=0.96+0.04*(std::f32::consts::TAU*phase/circle_hold).cos();
        return Pose {shape:2.0,rotation:0.0,scale,animate:true,wake:None};
    }
    let active=phase-circle_hold;
    let smooth=|x:f32|{let x=x.clamp(0.0,1.0);x*x*(3.0-2.0*x)};
    let shape=if active<duration {2.0-smooth(active/duration)}
        else if active<duration+triangle_hold {1.0}
        else {1.0+smooth((active-duration-triangle_hold)/duration)};
    let rotation=std::f32::consts::TAU*smooth(active/(2.0*duration+triangle_hold));
    Pose {shape,rotation,scale:1.0,animate:true,wake:None}
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn circle_turns_into_a_spinning_triangle_and_returns_without_a_square() {
        let period=5.0;let duration=0.6;let start=2.8;
        let hold=pose(1.0,period,duration);
        assert_eq!(hold.shape,2.0);assert!(hold.animate);assert!(hold.scale>=0.92 && hold.scale<1.0);
        let entering=pose(start+duration/2.0,period,duration);
        assert!((entering.shape-1.5).abs()<0.001);assert!(entering.rotation>0.0);
        let triangle=pose(start+duration+0.5,period,duration);
        assert_eq!(triangle.shape,1.0);assert!(triangle.rotation>entering.rotation);assert!(triangle.animate);
        let exiting=pose(period-duration/2.0,period,duration);
        assert!((exiting.shape-1.5).abs()<0.001);assert!(exiting.rotation>triangle.rotation);
        for step in 0..500 {assert!((1.0..=2.0).contains(&pose(step as f32/100.0,period,duration).shape));}
        let end=pose(period-0.00001,period,duration);
        assert!((end.shape-2.0).abs()<0.001);assert!((end.rotation-std::f32::consts::TAU).abs()<0.001);
        assert_eq!(pose(period,period,duration).shape,2.0);assert!(pose(period,period,duration).animate);assert_eq!(pose(period,period,duration).scale,1.0);
    }
}
