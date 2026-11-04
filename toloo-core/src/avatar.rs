//! Deterministic, cell-based avatars derived from Ed25519 public keys.
//!
//! The 5x5 grid is horizontally mirrored. The lower 15 bits of the first two
//! digest bytes select the independent cells, giving every public key a stable
//! visual identity shared by native and browser clients.

use anyhow::{Result, anyhow};
use sha2::{Digest, Sha256};

use crate::base64url;

const DOMAIN: &[u8] = b"toloo/avatar/v1";
const CELL_MASK: u16 = 0x7FFF;
// Exact fallback from the avatar spec; keeps the grid non-empty.
const ZERO_CELL_FALLBACK: u16 = 0b000_001_001_001_000;

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AvatarShape {
    RoundedSquare = 0,
    Circle = 1,
    Diamond = 2,
    Square = 3,
}

impl AvatarShape {
    pub fn from_u8(value: u8) -> Self {
        match value & 0b11 {
            0 => Self::RoundedSquare,
            1 => Self::Circle,
            2 => Self::Diamond,
            _ => Self::Square,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AvatarSpec {
    /// Lower 15 bits active; bit index is `row * 3 + mirror_col`.
    pub cells: u16,
    /// 0..3, matching [`AvatarShape`].
    pub shape: u8,
    /// 0..7, index into the shared 8-color avatar palette.
    pub palette: u8,
}

impl AvatarSpec {
    pub fn from_pubkey(pubkey: &[u8; 32]) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(DOMAIN);
        hasher.update(pubkey);
        let digest = hasher.finalize();

        let mut cells = u16::from_be_bytes([digest[0], digest[1]]) & CELL_MASK;
        if cells == 0 {
            cells = ZERO_CELL_FALLBACK;
        }

        Self {
            cells,
            shape: digest[2] & 0b11,
            palette: (digest[2] >> 2) & 0b111,
        }
    }

    /// Decode a base64url Ed25519 public key and derive its avatar.
    pub fn from_pubkey_b64(public_key: &str) -> Result<Self> {
        let bytes = base64url::decode(public_key.trim())?;
        let pubkey: [u8; 32] = bytes
            .as_slice()
            .try_into()
            .map_err(|_| anyhow!("Ed25519 public key must be 32 bytes"))?;
        Ok(Self::from_pubkey(&pubkey))
    }

    #[inline]
    pub fn is_active(&self, row: usize, col: usize) -> bool {
        let mirror_col = if col < 3 { col } else { 4 - col };
        let bit_idx = row * 3 + mirror_col;
        ((self.cells >> bit_idx) & 1) == 1
    }

    /// Expand the mirrored 15-bit mask into a flat row-major 25-cell grid.
    pub fn grid_25(&self) -> [bool; 25] {
        let mut out = [false; 25];
        for (index, cell) in out.iter_mut().enumerate() {
            *cell = self.is_active(index / 5, index % 5);
        }
        out
    }

    /// Single compound SVG path on a 100x100 viewbox.
    pub fn to_svg_path(&self) -> String {
        use std::fmt::Write;

        let mut d = String::with_capacity(1024);
        let step = 20.0_f32;
        let size = 17.0_f32;
        let pad = 1.5_f32;

        for row in 0..5 {
            for col in 0..5 {
                if !self.is_active(row, col) {
                    continue;
                }
                let x = pad + col as f32 * step;
                let y = pad + row as f32 * step;
                let cx = x + size * 0.5;
                let cy = y + size * 0.5;
                let radius = size * 0.5;

                match AvatarShape::from_u8(self.shape) {
                    AvatarShape::RoundedSquare => {
                        let r = 4.5_f32;
                        let right = x + size;
                        let bottom = y + size;
                        let _ = write!(
                            d,
                            "M{:.1} {:.1}H{:.1}A{:.1} {:.1} 0 0 1 {:.1} {:.1}V{:.1}A{:.1} {:.1} 0 0 1 {:.1} {:.1}H{:.1}A{:.1} {:.1} 0 0 1 {:.1} {:.1}V{:.1}A{:.1} {:.1} 0 0 1 {:.1} {:.1}Z",
                            x + r,
                            y,
                            right - r,
                            r,
                            r,
                            right,
                            y + r,
                            bottom - r,
                            r,
                            r,
                            right - r,
                            bottom,
                            x + r,
                            r,
                            r,
                            x,
                            bottom - r,
                            y + r,
                            r,
                            r,
                            x + r,
                            y
                        );
                    }
                    AvatarShape::Circle => {
                        let _ = write!(
                            d,
                            "M{:.1} {:.1}a{:.1} {:.1} 0 1 0 {:.1} 0a{:.1} {:.1} 0 1 0 -{:.1} 0Z",
                            cx - radius,
                            cy,
                            radius,
                            radius,
                            size,
                            radius,
                            radius,
                            size
                        );
                    }
                    AvatarShape::Diamond => {
                        let _ = write!(
                            d,
                            "M{:.1} {:.1}L{:.1} {:.1}L{:.1} {:.1}L{:.1} {:.1}Z",
                            cx,
                            y,
                            x + size,
                            cy,
                            cx,
                            y + size,
                            x,
                            cy
                        );
                    }
                    AvatarShape::Square => {
                        let _ = write!(
                            d,
                            "M{:.1} {:.1}h{:.1}v{:.1}h-{:.1}Z",
                            x, y, size, size, size
                        );
                    }
                }
            }
        }
        d
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // RFC 8032 test public key, base64url encoded.
    const TEST_KEY: &str = "11qYAYKxCrfVS_7TyWQHOg7hcvPapiMlrwIaaPcHURo";

    #[test]
    fn derives_a_stable_spec() {
        let first = AvatarSpec::from_pubkey_b64(TEST_KEY).unwrap();
        let second = AvatarSpec::from_pubkey_b64(TEST_KEY).unwrap();
        assert_eq!(first, second);
        assert_ne!(first.cells, 0);
    }

    #[test]
    fn matches_the_reference_vector() {
        // Independently computed from SHA256("toloo/avatar/v1" || pubkey).
        let spec = AvatarSpec::from_pubkey_b64(TEST_KEY).unwrap();
        assert_eq!(spec.cells, 0x24FA);
        assert_eq!(spec.shape, 0);
        assert_eq!(spec.palette, 7);

        let expected = [".#.#.", "#####", "##.##", ".#.#.", ".#.#."];
        let grid = spec.grid_25();
        for (row, line) in expected.iter().enumerate() {
            for (col, ch) in line.chars().enumerate() {
                assert_eq!(grid[row * 5 + col], ch == '#', "cell ({row}, {col})");
            }
        }
    }

    fn pubkey_with_zero_cells() -> [u8; 32] {
        let mut candidate = [0u8; 32];
        loop {
            let mut hasher = Sha256::new();
            hasher.update(DOMAIN);
            hasher.update(candidate);
            let digest = hasher.finalize();
            if u16::from_be_bytes([digest[0], digest[1]]) & CELL_MASK == 0 {
                return candidate;
            }
            for byte in candidate.iter_mut() {
                *byte = byte.wrapping_add(1);
                if *byte != 0 {
                    break;
                }
            }
        }
    }

    #[test]
    fn never_returns_an_empty_grid() {
        let spec = AvatarSpec::from_pubkey(&pubkey_with_zero_cells());
        assert_eq!(spec.cells, ZERO_CELL_FALLBACK);
        assert!(spec.grid_25().iter().any(|cell| *cell));
    }

    #[test]
    fn expands_mirrored_cells() {
        let spec = AvatarSpec {
            cells: 0b000_000_000_000_001,
            shape: 0,
            palette: 0,
        };
        assert!(spec.is_active(0, 0));
        assert!(spec.is_active(0, 4));
        assert!(!spec.is_active(0, 1));
        assert_eq!(spec.grid_25().iter().filter(|cell| **cell).count(), 2);
    }

    #[test]
    fn emits_a_compound_path() {
        let spec = AvatarSpec::from_pubkey_b64(TEST_KEY).unwrap();
        let path = spec.to_svg_path();
        assert!(!path.is_empty());
        assert!(path.starts_with('M'));
        assert!(path.ends_with('Z'));
        assert_eq!(
            path.matches('M').count(),
            spec.grid_25().iter().filter(|cell| **cell).count()
        );
    }
}
