use core::convert::Infallible;

use chrono::NaiveDateTime;
use embedded_graphics::{
    mono_font::{
        ascii::{FONT_10X20, FONT_6X10},
        MonoTextStyle,
    },
    pixelcolor::BinaryColor,
    prelude::*,
    primitives::{Circle, Line, PrimitiveStyle, Rectangle, Triangle},
    text::{Alignment, Baseline, Text, TextStyleBuilder},
};

use crate::{
    config::{
        RefreshTestGlyphs, RefreshTestRegion, RefreshTestSettings, RefreshTestSize, Settings,
    },
    weather::{description, Weather},
};

pub const FRAME_BYTES: usize = 240 * 416 / 8;

pub struct LandscapeFrame {
    bytes: Box<[u8; FRAME_BYTES]>,
}

impl LandscapeFrame {
    pub fn new() -> Self {
        Self {
            bytes: Box::new([0xff; FRAME_BYTES]),
        }
    }

    pub fn bytes(&self) -> &[u8] {
        self.bytes.as_slice()
    }

    /// Extracts an already-rendered byte-aligned native controller window.
    pub fn native_window_bytes(
        &self,
        x_start: usize,
        y_start: usize,
        x_end: usize,
        y_end: usize,
    ) -> Vec<u8> {
        debug_assert_eq!(x_start % 8, 0);
        debug_assert_eq!((x_end + 1) % 8, 0);
        let first_byte = x_start / 8;
        let row_bytes = (x_end - x_start + 1) / 8;
        let mut window = Vec::with_capacity(row_bytes * (y_end - y_start + 1));
        for y in y_start..=y_end {
            let offset = y * 30 + first_byte;
            window.extend_from_slice(&self.bytes[offset..offset + row_bytes]);
        }
        window
    }

    pub fn render(&mut self, settings: &Settings, weather: &Weather, now: Option<NaiveDateTime>) {
        self.clear(BinaryColor::Off).ok();

        let small = MonoTextStyle::new(&FONT_6X10, BinaryColor::On);
        let medium = MonoTextStyle::new(&FONT_10X20, BinaryColor::On);
        let centered = TextStyleBuilder::new()
            .alignment(Alignment::Center)
            .baseline(Baseline::Top)
            .build();
        let left = TextStyleBuilder::new().baseline(Baseline::Top).build();

        if let Some(now) = now {
            draw_time(self, &now.format("%H:%M").to_string());
            Text::with_text_style(
                &now.format("%A  %d %B %Y").to_string(),
                Point::new(208, 111),
                medium,
                centered,
            )
            .draw(self)
            .ok();
        } else {
            Text::with_text_style("SYNCING TIME", Point::new(208, 43), medium, centered)
                .draw(self)
                .ok();
            Text::with_text_style("Waiting for NTP", Point::new(208, 79), small, centered)
                .draw(self)
                .ok();
        }
        Rectangle::new(Point::new(16, 140), Size::new(384, 2))
            .into_styled(PrimitiveStyle::with_fill(BinaryColor::On))
            .draw(self)
            .ok();

        if weather.valid {
            let unit = if settings.metric { "C" } else { "F" };
            let temperature = format!("{:.0}", weather.temperature);
            draw_number(self, &temperature, Point::new(18, 153), 28, 48, 5, 7);
            Text::with_text_style(
                unit,
                Point::new(18 + temperature.len() as i32 * 35, 174),
                medium,
                left,
            )
            .draw(self)
            .ok();
            Text::with_text_style(
                description(weather.code),
                Point::new(142, 153),
                medium,
                left,
            )
            .draw(self)
            .ok();
            Text::with_text_style(
                &format!("Feels {:.0} {}", weather.apparent, unit),
                Point::new(144, 181),
                small,
                left,
            )
            .draw(self)
            .ok();
            let speed = if settings.metric { "km/h" } else { "mph" };
            Text::with_text_style(
                &format!(
                    "Humidity {:.0}%    Wind {:.0} {}",
                    weather.humidity, weather.wind, speed
                ),
                Point::new(144, 207),
                small,
                left,
            )
            .draw(self)
            .ok();
        } else {
            Text::with_text_style(
                "Weather unavailable",
                Point::new(208, 174),
                medium,
                centered,
            )
            .draw(self)
            .ok();
        }
    }

    pub fn render_test(&mut self) {
        self.clear(BinaryColor::Off).ok();
        Rectangle::new(Point::new(1, 1), Size::new(414, 238))
            .into_styled(PrimitiveStyle::with_stroke(BinaryColor::On, 2))
            .draw(self)
            .ok();
        for x in [18, 318] {
            Rectangle::new(Point::new(x, 18), Size::new(80, 80))
                .into_styled(PrimitiveStyle::with_fill(BinaryColor::On))
                .draw(self)
                .ok();
        }
        let style = MonoTextStyle::new(&FONT_10X20, BinaryColor::On);
        let centered = TextStyleBuilder::new()
            .alignment(Alignment::Center)
            .baseline(Baseline::Top)
            .build();
        Text::with_text_style("DISPLAY TEST", Point::new(208, 119), style, centered)
            .draw(self)
            .ok();
    }

    pub fn render_refresh_test_base(&mut self) {
        self.clear(BinaryColor::Off).ok();
        let style = MonoTextStyle::new(&FONT_10X20, BinaryColor::On);
        let centered = TextStyleBuilder::new()
            .alignment(Alignment::Center)
            .baseline(Baseline::Top)
            .build();
        Text::with_text_style("REFRESH TEST", Point::new(208, 2), style, centered)
            .draw(self)
            .ok();
        for (label, rectangle) in [
            (
                "LEFT / LARGE",
                Rectangle::new(Point::new(8, 24), Size::new(192, 200)),
            ),
            (
                "RIGHT / SMALL",
                Rectangle::new(Point::new(232, 64), Size::new(176, 112)),
            ),
        ] {
            rectangle
                .into_styled(PrimitiveStyle::with_stroke(BinaryColor::On, 2))
                .draw(self)
                .ok();
            Text::with_text_style(
                label,
                Point::new(rectangle.center().x, rectangle.top_left.y + 8),
                MonoTextStyle::new(&FONT_6X10, BinaryColor::On),
                centered,
            )
            .draw(self)
            .ok();
        }
    }

    pub fn render_bench_base(&mut self, config: &RefreshTestSettings) {
        if config.region == RefreshTestRegion::Preset {
            self.render_refresh_test_base();
            return;
        }
        self.clear(BinaryColor::Off).ok();
        // Reference grid makes unintended changes outside the selected window visible.
        for x in (0..416).step_by(32) {
            for y in (0..240).step_by(32) {
                Line::new(Point::new(x, y), Point::new(x + 5, y))
                    .into_styled(PrimitiveStyle::with_stroke(BinaryColor::On, 1))
                    .draw(self)
                    .ok();
            }
        }
    }

    pub fn render_bench_patch(&mut self, config: &RefreshTestSettings, item: u16, rect: Rectangle) {
        let kind = if config.glyphs == RefreshTestGlyphs::Mixed {
            match item % 4 {
                0 => RefreshTestGlyphs::Digits,
                1 => RefreshTestGlyphs::All,
                2 => RefreshTestGlyphs::Shapes,
                _ => RefreshTestGlyphs::Shades,
            }
        } else {
            config.glyphs
        };
        let item = if config.glyphs == RefreshTestGlyphs::Mixed {
            item / 4
        } else {
            item
        };
        let large = config.size == RefreshTestSize::Large;
        if kind == RefreshTestGlyphs::All {
            rect.into_styled(PrimitiveStyle::with_fill(BinaryColor::Off))
                .draw(self)
                .ok();
            let scale = if large { 7 } else { 1 };
            let mut scaled = ScaledDrawTarget {
                target: self,
                origin: rect.center() - Point::new(5 * scale, 10 * scale),
                scale,
                clip: rect,
            };
            let encoded = [33 + (item % 94) as u8];
            let glyph = core::str::from_utf8(&encoded).unwrap();
            Text::with_baseline(
                glyph,
                Point::zero(),
                MonoTextStyle::new(&FONT_10X20, BinaryColor::On),
                Baseline::Top,
            )
            .draw(&mut scaled)
            .ok();
            return;
        }
        let mut target = self.clipped(&rect);
        rect.into_styled(PrimitiveStyle::with_fill(BinaryColor::Off))
            .draw(&mut target)
            .ok();
        match kind {
            RefreshTestGlyphs::Digits => {
                let (w, h, t) = if large { (100, 160, 14) } else { (20, 32, 3) };
                draw_digit(
                    &mut target,
                    (item % 10) as u8,
                    rect.center() - Point::new(w as i32 / 2, h as i32 / 2),
                    w,
                    h,
                    t,
                );
            }
            RefreshTestGlyphs::Shapes => {
                let style = if item % 2 == 0 {
                    PrimitiveStyle::with_fill(BinaryColor::On)
                } else {
                    PrimitiveStyle::with_stroke(BinaryColor::On, 3)
                };
                let p = rect.top_left;
                let end = p + Point::new(rect.size.width as i32 - 1, rect.size.height as i32 - 1);
                match item % 6 {
                    0 | 1 => {
                        rect.into_styled(style).draw(&mut target).ok();
                    }
                    2 | 3 => {
                        let d = rect.size.width.min(rect.size.height);
                        Circle::new(rect.center() - Point::new(d as i32 / 2, d as i32 / 2), d)
                            .into_styled(style)
                            .draw(&mut target)
                            .ok();
                    }
                    _ => {
                        Triangle::new(
                            p,
                            Point::new(end.x, p.y),
                            Point::new(rect.center().x, end.y),
                        )
                        .into_styled(style)
                        .draw(&mut target)
                        .ok();
                        Line::new(p, end)
                            .into_styled(PrimitiveStyle::with_stroke(BinaryColor::On, 1))
                            .draw(&mut target)
                            .ok();
                    }
                }
            }
            RefreshTestGlyphs::Shades => {
                const BAYER: [[u16; 4]; 4] =
                    [[0, 8, 2, 10], [12, 4, 14, 6], [3, 11, 1, 9], [15, 7, 13, 5]];
                // 0..100% black coverage, then back to white on the next cycle.
                let level = item % 17;
                target
                    .draw_iter(rect.points().map(|p| {
                        Pixel(
                            p,
                            if BAYER[p.y as usize % 4][p.x as usize % 4] < level {
                                BinaryColor::On
                            } else {
                                BinaryColor::Off
                            },
                        )
                    }))
                    .ok();
            }
            _ => {}
        }
    }

    pub fn render_refresh_test_patch(&mut self, digit: u8, left: bool) {
        let rectangle = refresh_test_rectangle(left);
        clear_test_patch(self, rectangle);
        let (width, height, thickness) = if left { (100, 160, 14) } else { (56, 90, 8) };
        draw_digit(
            self,
            digit,
            Point::new(
                rectangle.center().x - width as i32 / 2,
                rectangle.center().y - height as i32 / 2,
            ),
            width,
            height,
            thickness,
        );
    }

    pub fn render_refresh_test_ascii(&mut self, glyph: char, left: bool) {
        let rectangle = refresh_test_rectangle(left);
        clear_test_patch(self, rectangle);
        let scale = if left { 7 } else { 4 };
        let origin = Point::new(
            rectangle.center().x - 5 * scale,
            rectangle.center().y - 10 * scale,
        );
        let mut scaled = ScaledDrawTarget {
            target: self,
            origin,
            scale,
            clip: rectangle,
        };
        let style = MonoTextStyle::new(&FONT_10X20, BinaryColor::On);
        let mut encoded = [0_u8; 4];
        let glyph = glyph.encode_utf8(&mut encoded);
        Text::with_baseline(glyph, Point::zero(), style, Baseline::Top)
            .draw(&mut scaled)
            .ok();
    }
}

struct ScaledDrawTarget<'a> {
    target: &'a mut LandscapeFrame,
    origin: Point,
    scale: i32,
    clip: Rectangle,
}

impl OriginDimensions for ScaledDrawTarget<'_> {
    fn size(&self) -> Size {
        Size::new(10, 20)
    }
}

impl DrawTarget for ScaledDrawTarget<'_> {
    type Color = BinaryColor;
    type Error = Infallible;

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Self::Color>>,
    {
        for Pixel(point, color) in pixels {
            for y in 0..self.scale {
                for x in 0..self.scale {
                    let position = Point::new(
                        self.origin.x + point.x * self.scale + x,
                        self.origin.y + point.y * self.scale + y,
                    );
                    if self.clip.contains(position) {
                        self.target
                            .draw_iter(core::iter::once(Pixel(position, color)))?;
                    }
                }
            }
        }
        Ok(())
    }
}

fn refresh_test_rectangle(left: bool) -> Rectangle {
    if left {
        Rectangle::new(Point::new(8, 24), Size::new(192, 200))
    } else {
        Rectangle::new(Point::new(232, 64), Size::new(176, 112))
    }
}

fn clear_test_patch(target: &mut LandscapeFrame, rectangle: Rectangle) {
    let mut target = target.clipped(&rectangle);
    rectangle
        .into_styled(PrimitiveStyle::with_fill(BinaryColor::Off))
        .draw(&mut target)
        .ok();
    rectangle
        .into_styled(PrimitiveStyle::with_stroke(BinaryColor::On, 3))
        .draw(&mut target)
        .ok();
}

fn draw_time(target: &mut LandscapeFrame, time: &str) {
    let mut x = 67;
    for character in time.chars() {
        if character == ':' {
            for y in [34, 70] {
                Rectangle::new(Point::new(x + 5, y), Size::new(10, 10))
                    .into_styled(PrimitiveStyle::with_fill(BinaryColor::On))
                    .draw(target)
                    .ok();
            }
            x += 26;
        } else if let Some(digit) = character.to_digit(10) {
            draw_digit(target, digit as u8, Point::new(x, 10), 56, 90, 8);
            x += 66;
        }
    }
}

fn draw_number(
    target: &mut LandscapeFrame,
    value: &str,
    origin: Point,
    width: u32,
    height: u32,
    thickness: u32,
    gap: i32,
) {
    let mut x = origin.x;
    for character in value.chars() {
        if character == '-' {
            Rectangle::new(
                Point::new(x + thickness as i32, origin.y + height as i32 / 2),
                Size::new(width - 2 * thickness, thickness),
            )
            .into_styled(PrimitiveStyle::with_fill(BinaryColor::On))
            .draw(target)
            .ok();
        } else if let Some(digit) = character.to_digit(10) {
            draw_digit(
                target,
                digit as u8,
                Point::new(x, origin.y),
                width,
                height,
                thickness,
            );
        }
        x += width as i32 + gap;
    }
}

fn draw_digit<T: DrawTarget<Color = BinaryColor>>(
    target: &mut T,
    digit: u8,
    origin: Point,
    width: u32,
    height: u32,
    thickness: u32,
) {
    // Segment bits are A, B, C, D, E, F, G from least to most significant.
    const SEGMENTS: [u8; 10] = [
        0b011_1111, 0b000_0110, 0b101_1011, 0b100_1111, 0b110_0110, 0b110_1101, 0b111_1101,
        0b000_0111, 0b111_1111, 0b110_1111,
    ];
    let horizontal = Size::new(width - 2 * thickness, thickness);
    let vertical = Size::new(thickness, height / 2 - thickness);
    let x = origin.x;
    let y = origin.y;
    let w = width as i32;
    let h = height as i32;
    let t = thickness as i32;
    let positions = [
        (Point::new(x + t, y), horizontal),
        (Point::new(x + w - t, y + t), vertical),
        (Point::new(x + w - t, y + h / 2), vertical),
        (Point::new(x + t, y + h - t), horizontal),
        (Point::new(x, y + h / 2), vertical),
        (Point::new(x, y + t), vertical),
        (Point::new(x + t, y + h / 2 - t / 2), horizontal),
    ];
    for (index, (point, size)) in positions.into_iter().enumerate() {
        if SEGMENTS[digit as usize] & (1 << index) != 0 {
            Rectangle::new(point, size)
                .into_styled(PrimitiveStyle::with_fill(BinaryColor::On))
                .draw(target)
                .ok();
        }
    }
}

impl OriginDimensions for LandscapeFrame {
    fn size(&self) -> Size {
        Size::new(416, 240)
    }
}

impl DrawTarget for LandscapeFrame {
    type Color = BinaryColor;
    type Error = Infallible;

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Self::Color>>,
    {
        for Pixel(point, color) in pixels {
            if point.x < 0 || point.y < 0 || point.x >= 416 || point.y >= 240 {
                continue;
            }
            // 90-degree clockwise logical landscape -> native 240x416 controller RAM.
            let native_x = 239 - point.y as usize;
            let native_y = point.x as usize;
            let index = native_y * 30 + native_x / 8;
            let mask = 1 << (7 - native_x % 8);
            match color {
                BinaryColor::On => self.bytes[index] &= !mask,
                BinaryColor::Off => self.bytes[index] |= mask,
            }
        }
        Ok(())
    }
}
