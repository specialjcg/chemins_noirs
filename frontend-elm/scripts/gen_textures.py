#!/usr/bin/env python3
"""Generate tileable terrain textures for the 3D orienteering game."""

import random
import math
from PIL import Image, ImageFilter

OUT = "public/textures"
SIZE = 128  # power of 2, small enough for perf


def noise_texture(size, base_rgb, variation, seed=42):
    """Create a noisy tileable texture."""
    random.seed(seed)
    img = Image.new("RGB", (size, size))
    pixels = img.load()
    for y in range(size):
        for x in range(size):
            r = max(0, min(255, base_rgb[0] + random.randint(-variation, variation)))
            g = max(0, min(255, base_rgb[1] + random.randint(-variation, variation)))
            b = max(0, min(255, base_rgb[2] + random.randint(-variation, variation)))
            pixels[x, y] = (r, g, b)
    # Blur slightly for smoother tiling
    img = img.filter(ImageFilter.GaussianBlur(radius=1.2))
    return img


def make_tileable(img):
    """Make texture seamlessly tileable by blending edges."""
    s = img.size[0]
    half = s // 2
    result = Image.new("RGB", (s, s))
    px_in = img.load()
    px_out = result.load()

    for y in range(s):
        for x in range(s):
            # Shift by half to move seams to center
            sx = (x + half) % s
            sy = (y + half) % s
            px_out[x, y] = px_in[sx, sy]

    # Blend center cross with original
    blend = 20
    for y in range(s):
        for x in range(s):
            dx = min(abs(x - half), abs(x - half + s), abs(x - half - s))
            dy = min(abs(y - half), abs(y - half + s), abs(y - half - s))
            d = min(dx, dy)
            if d < blend:
                t = d / blend
                r1, g1, b1 = px_out[x, y]
                r2, g2, b2 = px_in[x, y]
                px_out[x, y] = (
                    int(r1 * t + r2 * (1 - t)),
                    int(g1 * t + g2 * (1 - t)),
                    int(b1 * t + b2 * (1 - t)),
                )
    return result


def grass():
    """Green grass texture."""
    img = noise_texture(SIZE, (75, 135, 45), 25, seed=1)
    # Add darker blades
    px = img.load()
    random.seed(100)
    for _ in range(SIZE * 4):
        x, y = random.randint(0, SIZE - 1), random.randint(0, SIZE - 1)
        r, g, b = px[x, y]
        px[x, y] = (max(0, r - 20), max(0, g - 15), max(0, b - 10))
    img = img.filter(ImageFilter.GaussianBlur(radius=0.8))
    return make_tileable(img)


def dirt():
    """Brown dirt/trail texture."""
    img = noise_texture(SIZE, (165, 125, 75), 30, seed=2)
    # Add small pebbles (lighter spots)
    px = img.load()
    random.seed(200)
    for _ in range(SIZE * 2):
        x, y = random.randint(0, SIZE - 1), random.randint(0, SIZE - 1)
        r, g, b = px[x, y]
        px[x, y] = (min(255, r + 30), min(255, g + 25), min(255, b + 20))
    img = img.filter(ImageFilter.GaussianBlur(radius=0.6))
    return make_tileable(img)


def gravel():
    """Gray gravel/road texture."""
    img = noise_texture(SIZE, (140, 140, 135), 25, seed=3)
    px = img.load()
    random.seed(300)
    for _ in range(SIZE * 3):
        x, y = random.randint(0, SIZE - 1), random.randint(0, SIZE - 1)
        r, g, b = px[x, y]
        d = random.choice([-25, -15, 15, 25])
        px[x, y] = (max(0, min(255, r + d)), max(0, min(255, g + d)), max(0, min(255, b + d)))
    img = img.filter(ImageFilter.GaussianBlur(radius=0.5))
    return make_tileable(img)


def asphalt():
    """Dark asphalt texture."""
    img = noise_texture(SIZE, (55, 55, 55), 12, seed=4)
    # Subtle cracks
    px = img.load()
    random.seed(400)
    for _ in range(SIZE):
        x, y = random.randint(0, SIZE - 1), random.randint(0, SIZE - 1)
        r, g, b = px[x, y]
        px[x, y] = (max(0, r - 15), max(0, g - 15), max(0, b - 15))
    img = img.filter(ImageFilter.GaussianBlur(radius=0.4))
    return make_tileable(img)


def forest_floor():
    """Dark green forest floor."""
    img = noise_texture(SIZE, (35, 90, 30), 20, seed=5)
    px = img.load()
    random.seed(500)
    # Fallen leaves (brown spots)
    for _ in range(SIZE * 2):
        x, y = random.randint(0, SIZE - 1), random.randint(0, SIZE - 1)
        r, g, b = px[x, y]
        px[x, y] = (min(255, r + 40), g, max(0, b - 10))
    img = img.filter(ImageFilter.GaussianBlur(radius=1.0))
    return make_tileable(img)


def vineyard_soil():
    """Brown vineyard soil with row pattern."""
    img = noise_texture(SIZE, (130, 100, 65), 20, seed=6)
    px = img.load()
    # Horizontal row stripes
    for y in range(SIZE):
        stripe = (y % 16) < 5
        for x in range(SIZE):
            r, g, b = px[x, y]
            if stripe:
                px[x, y] = (max(0, r - 25), max(0, g - 20), max(0, b - 15))
            else:
                px[x, y] = (min(255, r + 10), min(255, g + 15), min(255, b + 5))
    img = img.filter(ImageFilter.GaussianBlur(radius=0.8))
    return make_tileable(img)


if __name__ == "__main__":
    textures = {
        "grass": grass(),
        "dirt": dirt(),
        "gravel": gravel(),
        "asphalt": asphalt(),
        "forest": forest_floor(),
        "vineyard": vineyard_soil(),
    }
    for name, img in textures.items():
        path = f"{OUT}/{name}.png"
        img.save(path, "PNG", optimize=True)
        print(f"  {path} ({img.size[0]}x{img.size[1]})")
    print(f"Done: {len(textures)} textures")
