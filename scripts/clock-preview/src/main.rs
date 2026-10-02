#![allow(dead_code)]
mod config {
    #[derive(Clone, Copy, PartialEq, Eq)] pub enum RefreshTestSize { Large, Small }
    #[derive(Clone, Copy, PartialEq, Eq)] pub enum RefreshTestRegion { Preset, Custom, Sweep, Random, Grid, TextPage, Patterns }
    #[derive(Clone, Copy, PartialEq, Eq)] pub enum RefreshTestGlyphs { Digits, All, Shapes, Shades, Mixed, Text }
    #[derive(Clone)] pub struct RefreshTestSettings { pub region: RefreshTestRegion, pub size: RefreshTestSize, pub glyphs: RefreshTestGlyphs }
    pub struct Settings { pub metric: bool }
}
mod weather {
    pub struct Weather { pub valid: bool, pub temperature: f64, pub apparent: f64, pub humidity: f64, pub wind: f64, pub code: u16, pub is_day: bool, pub stale: bool }
    pub fn description(_: u16) -> &'static str { "Partly cloudy" }
}
#[path = "../../../src/display.rs"] mod display;
#[path = "../../../src/bench_geometry.rs"] mod bench_geometry;
#[path = "../../../src/clock_policy.rs"] mod clock_policy;

fn main() {
    let settings = config::Settings { metric: true };
    let weather = weather::Weather { valid: true, temperature: 27., apparent: 29., humidity: 63., wind: 12., code: 2, is_day: true, stale: false };
    let mut frame = display::LandscapeFrame::new();
    let time = chrono::NaiveDate::from_ymd_opt(2026,10,2).unwrap().and_hms_opt(12,34,0).unwrap();
    frame.render(&settings,&weather,Some(time));
    // Rotate exactly like the physical landscape framebuffer, producing PBM.
    let mut bitmap = b"P4\n416 240\n".to_vec();
    for y in 0..240 {
        for byte_x in 0..52 {
            let mut byte = 0_u8;
            for bit in 0..8 {
                let x = byte_x*8+bit;
                let nx = 239-y;
                if frame.bytes()[x*30+nx/8] & (1 << (7-nx%8)) == 0 { byte |= 1 << (7-bit); }
            }
            bitmap.push(byte);
        }
    }
    std::fs::write("target/clock-preview.pbm",bitmap).unwrap();
    let previous = frame.bytes().to_vec();
    frame.render(&settings,&weather,Some(time+chrono::Duration::minutes(1)));
    let (_,_,_,_) = clock_policy::dirty_window(&previous,frame.bytes()).unwrap();
    // A minute change must leave date + weather rows untouched.
    for logical_y in 132..240 {
        for x in 0..416 {
            let nx = 239-logical_y;
            let mask = 1 << (7-nx%8);
            assert_eq!(previous[x*30+nx/8]&mask,frame.bytes()[x*30+nx/8]&mask);
        }
    }
    println!("Preview rendered; minute update preserves date/weather.");
    use embedded_graphics::{prelude::*, primitives::Rectangle};
    frame.render_text_page(config::RefreshTestSize::Small);
    let previous = frame.bytes().to_vec();
    let (x,y,w,h) = bench_geometry::text_patch(6,10,1,4);
    let rectangle = Rectangle::new(Point::new(x as i32,y as i32),Size::new(w,h));
    frame.render_text_patch(config::RefreshTestSize::Small,4,rectangle);
    for ly in 0..240 {
        for lx in 0..416 {
            if !rectangle.contains(Point::new(lx as i32,ly as i32)) {
                let nx = 239-ly;
                let mask = 1 << (7-nx%8);
                assert_eq!(previous[lx*30+nx/8]&mask,frame.bytes()[lx*30+nx/8]&mask);
            }
        }
    }
    println!("Text patch preserves every neighboring pixel.");
}
