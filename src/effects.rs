use std::{
    env,
    ffi::CStr,
    io::{self, Write},
    ops::Range,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use crate::{
    prompt::Layout,
    terminal::{self, Terminal, Viewport},
};

type Color = [u8; 3];

#[derive(Clone, Copy, Debug)]
struct Point {
    x: f64,
    y: f64,
    z: f64,
}

impl Point {
    const fn new(x: f64, y: f64, z: f64) -> Self {
        Self { x, y, z }
    }

    fn rotate(self, angles: Self) -> Self {
        let (sa, ca) = angles.x.sin_cos();
        let (sb, cb) = angles.y.sin_cos();
        let (sc, cc) = angles.z.sin_cos();
        let (y, z) = (self.y * ca - self.z * sa, self.y * sa + self.z * ca);
        let (x, z) = (self.x * cb + z * sb, -self.x * sb + z * cb);
        Self::new(x * cc - y * sc, x * sc + y * cc, z)
    }

    fn lerp(self, other: Self, progress: f64) -> Self {
        Self::new(
            self.x + (other.x - self.x) * progress,
            self.y + (other.y - self.y) * progress,
            self.z + (other.z - self.z) * progress,
        )
    }
}

struct Fragment {
    glyphs: Range<usize>,
    target: Point,
    start: Point,
    spin: Point,
    bend: f64,
    color: Color,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Ink {
    Empty,
    Block,
    Glyph(usize),
    Continuation,
}

#[derive(Clone, Copy)]
struct Pixel {
    depth: f64,
    color: Color,
    ink: Ink,
}

const EMPTY: Pixel = Pixel {
    depth: f64::INFINITY,
    color: [0; 3],
    ink: Ink::Empty,
};

struct Canvas {
    viewport: Viewport,
    pixels: Vec<Pixel>,
}

impl Canvas {
    fn new(viewport: Viewport) -> Self {
        Self {
            viewport,
            pixels: vec![EMPTY; viewport.area()],
        }
    }

    fn pixel(&mut self, x: i32, y: i32, depth: f64, color: Color, ink: Ink) {
        if x < 0 || y < 0 || x >= self.viewport.columns as i32 || y >= self.viewport.rows as i32 {
            return;
        }
        let old = &mut self.pixels[y as usize * self.viewport.columns + x as usize];
        if depth < old.depth {
            *old = Pixel { depth, color, ink };
        }
    }

    fn triangle(&mut self, [a, b, c]: [Point; 3], color: Color) {
        let cross = (b.y - c.y) * (a.x - c.x) + (c.x - b.x) * (a.y - c.y);
        if cross.abs() < 0.01 {
            return;
        }
        let left = a.x.min(b.x).min(c.x).floor().max(0.0) as usize;
        let right =
            a.x.max(b.x)
                .max(c.x)
                .ceil()
                .min(self.viewport.columns as f64)
                .max(0.0) as usize;
        let top = a.y.min(b.y).min(c.y).floor().max(0.0) as usize;
        let bottom =
            a.y.max(b.y)
                .max(c.y)
                .ceil()
                .min(self.viewport.rows as f64)
                .max(0.0) as usize;
        for y in top..bottom {
            if terminal::cancelled() {
                return;
            }
            for x in left..right {
                let u = ((b.y - c.y) * (x as f64 + 0.5 - c.x)
                    + (c.x - b.x) * (y as f64 + 0.5 - c.y))
                    / cross;
                let v = ((c.y - a.y) * (x as f64 + 0.5 - c.x)
                    + (a.x - c.x) * (y as f64 + 0.5 - c.y))
                    / cross;
                let w = 1.0 - u - v;
                if u.min(v).min(w) >= 0.0 {
                    self.pixel(
                        x as i32,
                        y as i32,
                        u * a.z + v * b.z + w * c.z,
                        color,
                        Ink::Block,
                    );
                }
            }
        }
    }

    fn serialize(&self, layout: &Layout, output: &mut Vec<u8>) {
        for (index, pixel) in self.pixels.iter().enumerate() {
            let text = match pixel.ink {
                Ink::Empty | Ink::Continuation => continue,
                Ink::Block => "█",
                Ink::Glyph(index) => &layout.glyphs[index].text,
            };
            layout.placement.position(
                output,
                index / self.viewport.columns,
                index % self.viewport.columns,
            );
            let [r, g, b] = pixel.color;
            write!(output, "\x1b[38;2;{r};{g};{b}m{text}").unwrap();
        }
    }
}

struct Random(u64);

impl Random {
    fn new() -> Self {
        let seed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64;
        Self((seed ^ (u64::from(std::process::id()) << 32)).max(1))
    }

    fn unit(&mut self) -> f64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 11) as f64 / (1_u64 << 53) as f64
    }

    fn between(&mut self, low: f64, high: f64) -> f64 {
        low + self.unit() * (high - low)
    }
}

pub struct Assembly {
    parts: Vec<Fragment>,
    canvas: Canvas,
}

impl Assembly {
    pub fn new(layout: &Layout, viewport: Viewport) -> Self {
        const PALETTE: [Color; 6] = [
            [30, 225, 255],
            [255, 65, 205],
            [175, 255, 50],
            [255, 180, 35],
            [170, 100, 255],
            [255, 95, 75],
        ];
        let mut chunks = Vec::new();
        let mut start = 0;
        for end in 1..=layout.glyphs.len() {
            let boundary = end == layout.glyphs.len() || {
                let previous = &layout.glyphs[end - 1];
                let next = &layout.glyphs[end];
                previous.cell.row != next.cell.row
                    || previous.cell.col + previous.width != next.cell.col
            };
            if boundary {
                while end - start > 4 {
                    chunks.push(start..start + 3);
                    start += 3;
                }
                chunks.push(start..end);
                start = end;
            }
        }
        let mut random = Random::new();
        let scale = viewport.scale();
        let parts = chunks
            .into_iter()
            .enumerate()
            .map(|(index, glyphs)| {
                let first = &layout.glyphs[glyphs.start];
                let last = &layout.glyphs[glyphs.end - 1];
                let span = (last.cell.col + last.width - first.cell.col) as f64;
                Fragment {
                    glyphs,
                    target: Point::new(
                        first.cell.col as f64 + span / 2.0,
                        (first.cell.row as f64 + 0.5) * 2.0,
                        0.0,
                    ),
                    start: Point::new(
                        random.between(0.0, viewport.columns as f64),
                        random.between(0.0, viewport.rows as f64 * 2.0),
                        random.between(-8.0, 26.0) * scale,
                    ),
                    spin: Point::new(
                        random.between(-2.5, 2.5),
                        random.between(-2.5, 2.5),
                        random.between(-2.5, 2.5),
                    ),
                    bend: (if random.unit() < 0.5 { -1.0 } else { 1.0 })
                        * random.between(4.0, 12.0)
                        * scale,
                    color: PALETTE[index % PALETTE.len()],
                }
            })
            .collect();
        Self {
            parts,
            canvas: Canvas::new(viewport),
        }
    }

    fn frame(&mut self, layout: &Layout, progress: f64, output: &mut Vec<u8>) {
        output.clear();
        output.extend_from_slice(b"\x1b[0m\x1b[2J");
        if progress >= 0.94 {
            layout.final_frame(output);
            return;
        }
        self.canvas.pixels.fill(EMPTY);
        let travel = (progress / 0.94).min(1.0);
        let eased = travel * travel * (3.0 - 2.0 * travel);
        let scene_scale = self.canvas.viewport.scale();
        let camera = 60.0 * scene_scale;
        let wave = (std::f64::consts::PI * travel).sin();
        for part in &self.parts {
            if terminal::cancelled() {
                return;
            }
            let mut center = part.start.lerp(part.target, eased);
            center.x += part.bend * wave;
            center.y -= part.bend * (2.0 * std::f64::consts::PI * travel).sin();
            center.z += 18.0 * scene_scale * wave;
            let angle = |a| a * (1.0 - travel).powf(1.3) + 0.35 * wave;
            let angles = Point::new(angle(part.spin.x), angle(part.spin.y), angle(part.spin.z));
            let scale = 0.24 + 2.5 * scene_scale * (1.0 - eased);
            let first = &layout.glyphs[part.glyphs.start];
            let last = &layout.glyphs[part.glyphs.end - 1];
            let span = (last.cell.col + last.width - first.cell.col) as f64;
            let half_width = (span + 2.0) / 2.0;
            let mut vertices = [Point::new(0.0, 0.0, 0.0); 8];
            for (index, vertex) in vertices.iter_mut().enumerate() {
                let (x, y) = [
                    (-half_width, -2.3),
                    (half_width, -2.3),
                    (half_width, 2.3),
                    (-half_width, 2.3),
                ][index % 4];
                let z = if index < 4 { -0.75 } else { 0.75 };
                let point = Point::new(x * scale, y * scale, z * scale).rotate(angles);
                let perspective = camera / (camera + center.z + point.z);
                *vertex = Point::new(
                    center.x + point.x * perspective,
                    (center.y + point.y * perspective) / 2.0,
                    center.z + point.z,
                );
            }
            const FACES: [[usize; 4]; 6] = [
                [0, 1, 2, 3],
                [4, 7, 6, 5],
                [0, 4, 5, 1],
                [3, 2, 6, 7],
                [0, 3, 7, 4],
                [1, 5, 6, 2],
            ];
            const NORMALS: [Point; 6] = [
                Point::new(0.0, 0.0, -1.0),
                Point::new(0.0, 0.0, 1.0),
                Point::new(0.0, -1.0, 0.0),
                Point::new(0.0, 1.0, 0.0),
                Point::new(-1.0, 0.0, 0.0),
                Point::new(1.0, 0.0, 0.0),
            ];
            for (index, face) in FACES.into_iter().enumerate() {
                let normal = NORMALS[index].rotate(angles);
                let light = (-0.35 * normal.x - 0.45 * normal.y - 0.8 * normal.z).max(0.0);
                let material = if index == 0 { 1.0 } else { 0.62 };
                let shade = material
                    * (0.55 + 0.45 * light)
                    * (1.0 - center.z.max(0.0) / (130.0 * scene_scale));
                let color = part
                    .color
                    .map(|c| (f64::from(c) * shade).clamp(0.0, 255.0) as u8);
                self.canvas.triangle(
                    [vertices[face[0]], vertices[face[1]], vertices[face[2]]],
                    color,
                );
                self.canvas.triangle(
                    [vertices[face[0]], vertices[face[2]], vertices[face[3]]],
                    color,
                );
            }
            for (i, j) in [(0, 1), (1, 2), (2, 3), (3, 0)] {
                let (a, b) = (vertices[i], vertices[j]);
                let steps = ((a.x - b.x).abs().max((a.y - b.y).abs()) * 2.0)
                    .ceil()
                    .max(1.0) as usize;
                for step in 0..=steps {
                    let point = a.lerp(b, step as f64 / steps as f64);
                    self.canvas.pixel(
                        point.x.round_ties_even() as i32,
                        point.y.round_ties_even() as i32,
                        point.z - 0.08,
                        [240, 255, 255],
                        Ink::Block,
                    );
                }
            }
            let front = &vertices[..4];
            let midx = front.iter().map(|v| v.x).sum::<f64>() / 4.0;
            let midy = front.iter().map(|v| v.y).sum::<f64>() / 4.0;
            let mut x = (midx - span / 2.0).round_ties_even() as i32;
            let y = midy.round_ties_even() as i32;
            let z = vertices.iter().map(|v| v.z).fold(f64::INFINITY, f64::min) - 0.2;
            for index in part.glyphs.clone() {
                let glyph = &layout.glyphs[index];
                if x >= 0 && x <= (self.canvas.viewport.columns - glyph.width) as i32 {
                    self.canvas.pixel(x, y, z, [255; 3], Ink::Glyph(index));
                    if glyph.width == 2 {
                        self.canvas.pixel(x + 1, y, z, [255; 3], Ink::Continuation);
                    }
                }
                x += glyph.width as i32;
            }
        }
        self.canvas.serialize(layout, output);
    }

    pub fn run(
        &mut self,
        layout: &Layout,
        terminal: &Terminal,
        output: &mut Vec<u8>,
    ) -> io::Result<()> {
        let started = Instant::now();
        for frame in 0..33 {
            self.frame(layout, f64::from(frame) / 32.0, output);
            terminal.write(output)?;
            terminal.wait_until(
                started + Duration::from_secs_f64(f64::from(frame + 1) * 1.05 / 33.0),
            )?;
        }
        Ok(())
    }
}

fn label(columns: usize) -> String {
    let username = ["LOGNAME", "USER", "LNAME", "USERNAME"]
        .into_iter()
        .find_map(|name| env::var(name).ok().filter(|s| !s.is_empty()))
        .unwrap_or_else(|| {
            let passwd = unsafe { libc::getpwuid(libc::getuid()) };
            if passwd.is_null() {
                String::new()
            } else {
                unsafe { CStr::from_ptr((*passwd).pw_name) }
                    .to_string_lossy()
                    .into_owned()
            }
        });
    let mut hostname = [0_u8; 256];
    unsafe { libc::gethostname(hostname.as_mut_ptr().cast(), hostname.len() - 1) };
    let hostname = String::from_utf8_lossy(&hostname);
    let hostname = hostname.split(['.', '\0']).next().unwrap_or("");
    format!("{username}@{hostname}")
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || "@._-".contains(*c))
        .take(columns)
        .collect()
}

pub fn shimmer(terminal: &Terminal, output: &mut Vec<u8>) -> io::Result<()> {
    let Viewport { columns, rows } = terminal.viewport();
    let label = label(columns);
    let palette = [23, 30, 37, 44, 87, 195, 231, 192, 154, 118, 41, 35, 29, 23];
    let background = "\x1b[48;5;233m";
    let band = (columns as f64 * 0.34).max(14.0);
    let slant = rows as f64 * 0.55;
    let started = Instant::now();
    for frame in 0..25 {
        let head = -band - slant + f64::from(frame) / 24.0 * (columns as f64 + 2.0 * band + slant);
        output.clear();
        for row in 0..rows {
            if terminal::cancelled() {
                return Err(io::ErrorKind::Interrupted.into());
            }
            write!(output, "\x1b[{};1H{background}\x1b[2K", row + 1).unwrap();
            let edge = head + row as f64 * 0.55;
            for (shade, color) in palette.iter().enumerate() {
                let left = ((edge + shade as f64 * band / palette.len() as f64) as i32).max(0);
                let right = ((edge + (shade + 1) as f64 * band / palette.len() as f64) as i32)
                    .min(columns as i32);
                if right > left {
                    write!(output, "\x1b[{};{}H\x1b[48;5;{color}m", row + 1, left + 1).unwrap();
                    output.resize(output.len() + (right - left) as usize, b' ');
                }
            }
        }
        if frame >= 25 * 5 / 13 {
            let color = if frame >= 25 * 9 / 13 { 231 } else { 87 };
            write!(
                output,
                "\x1b[{};{}H{background}\x1b[1;38;5;{color}m{label}\x1b[22m",
                rows.div_ceil(2).max(1),
                ((columns - label.len()) / 2 + 1).max(1)
            )
            .unwrap();
        }
        terminal.write(output)?;
        terminal
            .wait_until(started + Duration::from_secs_f64(f64::from(frame + 1) * 0.50 / 25.0))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prompt::{Cell, Placement};

    #[test]
    fn nearest_face_wins_and_wide_continuation_is_not_printed() {
        let viewport = Viewport::new(5, 3).unwrap();
        let layout = Layout::parse("界", viewport, Some(Cell { col: 0, row: 0 })).unwrap();
        let mut canvas = Canvas::new(viewport);
        canvas.pixel(1, 1, 2.0, [1, 2, 3], Ink::Block);
        canvas.pixel(1, 1, 1.0, [4, 5, 6], Ink::Glyph(0));
        canvas.pixel(2, 1, 1.0, [4, 5, 6], Ink::Continuation);
        canvas.pixel(1, 1, 3.0, [7, 8, 9], Ink::Block);
        let mut output = Vec::new();
        canvas.serialize(&layout, &mut output);
        assert_eq!(
            String::from_utf8(output).unwrap(),
            "\x1b[2;2H\x1b[38;2;4;5;6m界"
        );
    }

    #[test]
    fn triangle_fills_cells_and_clips_to_viewport() {
        let mut canvas = Canvas::new(Viewport::new(5, 3).unwrap());
        canvas.triangle(
            [
                Point::new(-10.0, -10.0, 1.0),
                Point::new(20.0, -10.0, 1.0),
                Point::new(0.0, 20.0, 1.0),
            ],
            [30, 225, 255],
        );
        assert!(canvas.pixels.iter().all(|pixel| pixel.ink == Ink::Block));
    }

    #[test]
    fn assembly_lands_at_native_cells_with_original_styles() {
        let viewport = Viewport::new(80, 24).unwrap();
        let layout = Layout::parse(
            "\x1b[32mPS1\x1b[0m$ ",
            viewport,
            Some(Cell { col: 12, row: 7 }),
        )
        .unwrap();
        let mut assembly = Assembly::new(&layout, viewport);
        let mut actual = Vec::new();
        assembly.frame(&layout, 1.0, &mut actual);
        let mut expected = b"\x1b[0m\x1b[2J".to_vec();
        layout.final_frame(&mut expected);
        assert_eq!(actual, expected);
        assert_eq!(
            assembly
                .parts
                .iter()
                .map(|part| part.glyphs.len())
                .sum::<usize>(),
            layout.glyphs.len()
        );
    }

    #[test]
    fn relative_pixels_restore_anchor_individually() {
        let mut output = Vec::new();
        let placement = Placement::Relative { anchor_row: 12 };
        placement.position(&mut output, 11, 4);
        placement.position(&mut output, 11, 5);
        assert_eq!(output, b"\x1b8\x1b[1A\x1b[4C\x1b8\x1b[1A\x1b[5C");
    }
}
