use super::geometry::endpoint_arc;
use super::{MAX_SEGMENTS, Result, XamlError};
use crate::canvas::{FillRule, Path2D};

pub(super) struct Numbers<'a> {
    s: &'a str,
    pos: usize,
}
impl<'a> Numbers<'a> {
    pub fn new(s: &'a str) -> Self {
        Self { s, pos: 0 }
    }
    pub fn space(&mut self) {
        while self.peek().is_some_and(|c| c.is_ascii_whitespace()) {
            self.pos += 1
        }
    }
    fn peek(&self) -> Option<u8> {
        self.s.as_bytes().get(self.pos).copied()
    }
    pub fn done(&mut self) -> bool {
        self.space();
        self.pos == self.s.len()
    }
    pub fn command(&mut self) -> Option<char> {
        self.space();
        let c = self.peek()?;
        if c.is_ascii_alphabetic() {
            self.pos += 1;
            Some(c as char)
        } else {
            None
        }
    }
    pub fn number(&mut self) -> Result<f64> {
        self.space();
        if self.peek() == Some(b',') {
            self.pos += 1;
            self.space();
        }
        let start = self.pos;
        if matches!(self.peek(), Some(b'+' | b'-')) {
            self.pos += 1
        }
        let mut digits = 0;
        while self.peek().is_some_and(|c| c.is_ascii_digit()) {
            digits += 1;
            self.pos += 1
        }
        if self.peek() == Some(b'.') {
            self.pos += 1;
            while self.peek().is_some_and(|c| c.is_ascii_digit()) {
                digits += 1;
                self.pos += 1
            }
        }
        if digits == 0 {
            return Err(self.error());
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.pos += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.pos += 1
            }
            let before = self.pos;
            while self.peek().is_some_and(|c| c.is_ascii_digit()) {
                self.pos += 1
            }
            if before == self.pos {
                return Err(self.error());
            }
        }
        let n = self.s[start..self.pos]
            .parse::<f64>()
            .map_err(|_| self.error())?;
        if !n.is_finite() || n.abs() > 1e100 {
            return Err(self.error());
        }
        Ok(n)
    }
    fn flag(&mut self) -> Result<bool> {
        self.space();
        if self.peek() == Some(b',') {
            self.pos += 1;
            self.space()
        }
        let c = self.peek();
        if matches!(c, Some(b'0' | b'1')) {
            self.pos += 1;
            Ok(c == Some(b'1'))
        } else {
            Err(self.error())
        }
    }
    fn error(&self) -> XamlError {
        XamlError::BadPathData(format!("at byte {}", self.pos))
    }
}
pub(super) fn numbers<const N: usize>(s: &str) -> Result<[f64; N]> {
    let mut n = Numbers::new(s);
    let mut out = [0.; N];
    if s.trim_start().starts_with(',') {
        return Err(n.error());
    }
    for v in &mut out {
        *v = n.number()?
    }
    if !n.done() {
        return Err(n.error());
    }
    Ok(out)
}
pub fn parse_path_markup(data: &str) -> Result<(Path2D, FillRule)> {
    if data.len() > 512 * 1024 {
        return Err(XamlError::TooLarge);
    }
    let mut n = Numbers::new(data);
    let mut rule = FillRule::EvenOdd;
    let mut cmd = n.command();
    if cmd == Some('F') {
        rule = if n.flag()? {
            FillRule::NonZero
        } else {
            FillRule::EvenOdd
        };
        cmd = n.command();
    }
    let mut p = Path2D::new();
    let mut current = [0., 0.];
    let mut start = current;
    let mut prev = ' ';
    let mut control = current;
    let mut first = true;
    let mut explicit = true;
    while !n.done() || cmd.is_some() {
        let c = cmd.take().ok_or_else(|| n.error())?;
        let upper = c.to_ascii_uppercase();
        let relative = c.is_ascii_lowercase();
        if first && upper != 'M' {
            return Err(n.error());
        }
        first = false;
        n.space();
        if explicit && n.peek() == Some(b',') {
            return Err(n.error());
        }
        let base = if relative { current } else { [0., 0.] };
        let point = |n: &mut Numbers<'_>| -> Result<[f64; 2]> {
            Ok([n.number()? + base[0], n.number()? + base[1]])
        };
        match upper {
            'M' => {
                current = point(&mut n)?;
                start = current;
                p.move_to(current[0], current[1]);
            }
            'L' => {
                current = point(&mut n)?;
                p.line_to(current[0], current[1]);
            }
            'H' => {
                current[0] = n.number()? + base[0];
                p.line_to(current[0], current[1]);
            }
            'V' => {
                current[1] = n.number()? + base[1];
                p.line_to(current[0], current[1]);
            }
            'C' | 'S' => {
                let a = if upper == 'C' {
                    point(&mut n)?
                } else if matches!(prev, 'C' | 'S') {
                    [2. * current[0] - control[0], 2. * current[1] - control[1]]
                } else {
                    current
                };
                let b = point(&mut n)?;
                current = point(&mut n)?;
                p.bezier_curve_to(a[0], a[1], b[0], b[1], current[0], current[1]);
                control = b;
            }
            'Q' | 'T' => {
                let a = if upper == 'Q' {
                    point(&mut n)?
                } else if matches!(prev, 'Q' | 'T') {
                    [2. * current[0] - control[0], 2. * current[1] - control[1]]
                } else {
                    current
                };
                current = point(&mut n)?;
                p.quadratic_curve_to(a[0], a[1], current[0], current[1]);
                control = a;
            }
            'A' => {
                let radii = [n.number()?, n.number()?];
                let angle = n.number()?;
                let large = n.flag()?;
                let clockwise = n.flag()?;
                let end = point(&mut n)?;
                endpoint_arc(&mut p, current, end, radii, angle, large, clockwise)?;
                current = end;
            }
            'Z' => {
                p.close_path();
                current = start;
            }
            _ => return Err(n.error()),
        }
        if p.verbs().len() > MAX_SEGMENTS {
            return Err(n.error());
        }
        prev = upper;
        if n.done() {
            break;
        }
        cmd = n.command();
        explicit = cmd.is_some();
        if cmd.is_none() {
            if upper == 'Z' {
                return Err(n.error());
            }
            cmd = Some(if upper == 'M' {
                if relative { 'l' } else { 'L' }
            } else {
                c
            });
        }
    }
    if first && !data.trim().is_empty() {
        return Err(n.error());
    }
    Ok((p, rule))
}
