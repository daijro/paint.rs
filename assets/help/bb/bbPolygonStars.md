# Polygon / Stars

Polygon / Stars draws outlined shapes ranging from a triangle to an almost circular 100-vertex polygon. Star connections and zigzag edges can also give a sticker-like outline.

Set Verticies first, then Star depth. Adjust Size/Orientation for size, the angle for rotation, and finally line width, color and Anti-Aliased.

Compare an eight-vertex shape at Star depth 0, 0.5 and 1.0: the polygon develops increasingly pointed connections. More vertices allow more distinct star-depth steps.

The original plugin uses Windows GDI+ for most strokes, with a separate pixel-line routine for one-pixel lines without antialiasing. paint.rs reproduces those stroke behaviors using its own renderer.

For questions about the original effects, visit the [Paint.NET forum](https://forums.getpaint.net/).
