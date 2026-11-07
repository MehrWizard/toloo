#!/usr/bin/env python3
"""Build the Toloo Benchmarks OG image (1200x630) from SVG + grain pass."""
import re
import subprocess
import tempfile
from pathlib import Path

from PIL import Image

ROOT = Path(__file__).resolve().parent
OUT = ROOT / 'benchmark-og.png'
LOGO = ROOT / 'toloo-logo.svg'
SVG_PATH = Path(tempfile.gettempdir()) / 'toloo-og.svg'
BASE_PNG = Path(tempfile.gettempdir()) / 'toloo-og-base.png'

# Inline the brand logo and paint it with the coral -> gold gradient.
logo_src = LOGO.read_text()
match = re.search(r'<svg[^>]*>(.*)</svg>', logo_src, re.S)
if match is None:
    raise SystemExit(f'invalid logo svg: {LOGO}')
logo_inner = match.group(1)
logo_inner = re.sub(r'\s+id="char-\d+"', '', logo_inner)
logo_inner = re.sub(r'\s+data-char="[^"]*"', '', logo_inner)
logo_inner = logo_inner.replace('fill="#f5a623"', 'fill="url(#logoBrand)"')
logo_inner = '\n'.join(line.rstrip() for line in logo_inner.splitlines() if line.strip())
logo_inner = logo_inner.replace('\n', '\n      ')

svg = f'''<svg xmlns="http://www.w3.org/2000/svg" width="1200" height="630" viewBox="0 0 1200 630">
  <defs>
    <linearGradient id="logoBrand" x1="0" y1="0" x2="1" y2="1">
      <stop offset="0" stop-color="#f43f5e"/>
      <stop offset="1" stop-color="#fbbf24"/>
    </linearGradient>
    <linearGradient id="textBrand" x1="0" y1="0" x2="1" y2="1">
      <stop offset="0" stop-color="#f43f5e"/>
      <stop offset="1" stop-color="#fbbf24"/>
    </linearGradient>
    <radialGradient id="coralGlow" gradientUnits="userSpaceOnUse" cx="300" cy="330" r="540">
      <stop offset="0" stop-color="#f43f5e" stop-opacity="0.22"/>
      <stop offset="0.5" stop-color="#f43f5e" stop-opacity="0.07"/>
      <stop offset="1" stop-color="#f43f5e" stop-opacity="0"/>
    </radialGradient>
    <radialGradient id="logoGlow" gradientUnits="userSpaceOnUse" cx="292" cy="300" r="250">
      <stop offset="0" stop-color="#f43f5e" stop-opacity="0.24"/>
      <stop offset="0.6" stop-color="#fbbf24" stop-opacity="0.06"/>
      <stop offset="1" stop-color="#fbbf24" stop-opacity="0"/>
    </radialGradient>
    <radialGradient id="goldGlow" gradientUnits="userSpaceOnUse" cx="1060" cy="140" r="560">
      <stop offset="0" stop-color="#fbbf24" stop-opacity="0.12"/>
      <stop offset="0.55" stop-color="#fbbf24" stop-opacity="0.03"/>
      <stop offset="1" stop-color="#fbbf24" stop-opacity="0"/>
    </radialGradient>
    <radialGradient id="vignette" cx="0.5" cy="0.44" r="0.78">
      <stop offset="0.55" stop-color="#000000" stop-opacity="0"/>
      <stop offset="1" stop-color="#000000" stop-opacity="0.42"/>
    </radialGradient>
  </defs>

  <rect width="1200" height="630" fill="#0b0809"/>
  <rect width="1200" height="630" fill="url(#coralGlow)"/>
  <rect width="1200" height="630" fill="url(#goldGlow)"/>
  <circle cx="292" cy="300" r="250" fill="url(#logoGlow)"/>

  <!-- brand mark -->
  <g transform="translate(84 100) scale(0.62)">
    {logo_inner}
  </g>

  <!-- vertical rule -->
  <line x1="566" y1="118" x2="566" y2="512" stroke="#34232a" stroke-width="1.2"/>
  <path d="M566 118h-12 M566 512h-12" stroke="#4a313a" stroke-width="1.2"/>

  <!-- type lockup -->
  <text x="622" y="302" font-family="Noto Sans" font-weight="700" font-size="112"
        letter-spacing="11" fill="url(#textBrand)">TOLOO</text>
  <text x="628" y="374" font-family="Noto Sans" font-weight="600" font-size="38"
        letter-spacing="10" fill="#f6eef0">BENCHMARKS</text>
  <line x1="628" y1="404" x2="1112" y2="404" stroke="#34232a" stroke-width="1"/>
  <text x="628" y="458" font-family="Noto Sans" font-weight="400" font-size="18"
        letter-spacing="0.2" fill="#b7a2aa">Protocol performance · Linux · Windows · macOS</text>

  <!-- corner ticks + frame -->
  <g stroke="url(#textBrand)" stroke-opacity="0.55" stroke-width="2" fill="none">
    <path d="M24 76V24h52"/>
    <path d="M1176 554v52h-52"/>
  </g>
  <rect width="1200" height="630" fill="url(#vignette)"/>
  <rect x="24" y="24" width="1152" height="582" rx="18" fill="none" stroke="#34232a" stroke-width="1.2"/>
</svg>'''

SVG_PATH.write_text(svg)
subprocess.run(['rsvg-convert', '-w', '1200', '-h', '630', '-o', str(BASE_PNG), str(SVG_PATH)], check=True)

base = Image.open(BASE_PNG).convert('RGB')
# Subtle film grain via Pillow noise, kept very low to survive PNG->PNG
noise = Image.effect_noise((1200, 630), 18).convert('L')
noise_rgba = Image.merge('RGB', (noise, noise, noise))
grain = Image.blend(base, noise_rgba, 0.035)
grain.save(OUT, format='PNG', optimize=True)
print('wrote', OUT, Image.open(OUT).size)
